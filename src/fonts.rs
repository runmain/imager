//! 系统字体：枚举可显示中文的字体族、按名称加载并设为界面首选字体。
//! 枚举与读取字体文件较慢（数百毫秒），只在后台线程调用 [`chinese_families`] / [`load_family`]。

use std::sync::{Arc, LazyLock, Mutex};

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use fontique::{Collection, CollectionOptions, FontStyle, FontWeight, FontWidth, SourceCache};

/// 判定“可显示中文”的样本字符：常用字 + 中文标点。
const CJK_SAMPLE: [char; 4] = ['中', '文', '图', '，'];
const UI_FONT_KEY: &str = "imager-ui-font";
const BOLD_FONT_KEY: &str = "imager-ui-font-bold";
/// “自动”字体时用于粗体的系统中文无衬线字体（按平台常见程度排列）。
const DEFAULT_BOLD_FAMILIES: [&str; 5] =
    ["PingFang SC", "Hiragino Sans GB", "Microsoft YaHei", "Noto Sans CJK SC", "Source Han Sans SC"];
/// 粗体字重：静态字体至少达到该字重才算真粗体；可变字体按此值设置 `wght` 轴。
const BOLD_WEIGHT: f32 = 600.0;

struct System {
    collection: Collection,
    cache: SourceCache,
}

/// 全进程共享一次系统字体枚举（首次访问时在调用线程上执行）。
static SYSTEM: LazyLock<Mutex<System>> = LazyLock::new(|| {
    Mutex::new(System {
        collection: Collection::new(CollectionOptions { shared: false, system_fonts: true }),
        cache: SourceCache::default(),
    })
});

fn system() -> &'static Mutex<System> {
    &SYSTEM
}

/// 系统中可显示中文的字体族名（去重、按名称排序；隐藏的 `.` 开头系统私有字体除外）。
pub fn chinese_families() -> Vec<String> {
    let mut guard = system().lock().unwrap_or_else(|p| p.into_inner());
    let System { collection, cache } = &mut *guard;
    let names: Vec<String> =
        collection.family_names().filter(|n| !n.starts_with('.')).map(str::to_string).collect();
    let mut out: Vec<String> = names
        .into_iter()
        .filter(|name| {
            collection.family_by_name(name).is_some_and(|family| {
                family.default_font().is_some_and(|font| {
                    font.load(Some(cache)).is_some_and(|blob| {
                        font.charmap_index()
                            .charmap(blob.as_ref())
                            .is_some_and(|map| CJK_SAMPLE.iter().all(|c| map.map(*c).is_some_and(|g| g != 0)))
                    })
                })
            })
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// 按字体族名加载其默认字形；找不到时返回 `None`。字体文件为内存映射，不复制。
pub fn load_family(name: &str) -> Option<FontData> {
    let mut guard = system().lock().unwrap_or_else(|p| p.into_inner());
    let System { collection, cache } = &mut *guard;
    let family = collection.family_by_name(name)?;
    let font = family.default_font()?;
    let (blob, _) = font.load(Some(cache))?.into_raw_parts();
    Some(FontData::from_blob(blob, font.index()))
}

/// 粗体字形：指定字体族（`None` 为系统中文无衬线字体）中字重最接近 600 的字体。
/// 静态字体字重不足（即没有真正的粗体）时返回 `None`；可变字体通过 `wght` 轴加粗。
pub fn load_bold(name: Option<&str>) -> Option<FontData> {
    let mut guard = system().lock().unwrap_or_else(|p| p.into_inner());
    let System { collection, cache } = &mut *guard;
    let names: Vec<&str> = match name {
        Some(n) => vec![n],
        None => DEFAULT_BOLD_FAMILIES.to_vec(),
    };
    names.into_iter().find_map(|n| {
        let family = collection.family_by_name(n)?;
        let font = family.match_font(FontWidth::NORMAL, FontStyle::Normal, FontWeight::new(BOLD_WEIGHT), false)?;
        let variable = font.has_weight_axis();
        if !variable && font.weight().value() < BOLD_WEIGHT - 50.0 {
            return None;
        }
        let (blob, _) = font.load(Some(cache))?.into_raw_parts();
        let data = FontData::from_blob(blob, font.index());
        Some(if variable {
            data.tweak(egui::FontTweak {
                coords: egui::epaint::text::VariationCoords::new([("wght", BOLD_WEIGHT)]),
                ..Default::default()
            })
        } else {
            data
        })
    })
}

/// 安装界面字体：`font` 为 `Some` 时放在比例字体首位、等宽字体第二位（英文代码仍等宽，中文用所选字体）；
/// `None` 恢复默认字体（中文经系统字体回退）。
/// 同时注册 Markdown 粗体字体族：`bold` 在首位，其后与比例字体相同；无粗体时即为常规字重。
pub fn install(ctx: &egui::Context, font: Option<FontData>, bold: Option<FontData>) {
    let mut defs = FontDefinitions::default();
    if let Some(data) = font {
        defs.font_data.insert(UI_FONT_KEY.into(), Arc::new(data));
        defs.families.entry(FontFamily::Proportional).or_default().insert(0, UI_FONT_KEY.into());
        let mono = defs.families.entry(FontFamily::Monospace).or_default();
        mono.insert(mono.len().min(1), UI_FONT_KEY.into());
    }
    let mut bold_list = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    if let Some(data) = bold {
        defs.font_data.insert(BOLD_FONT_KEY.into(), Arc::new(data));
        bold_list.insert(0, BOLD_FONT_KEY.into());
    }
    defs.families.insert(FontFamily::Name(crate::ui::markdown::BOLD_FAMILY.into()), bold_list);
    ctx.set_fonts(defs);
}

#[cfg(test)]
mod tests {
    use super::*;

    // AC-025：本机字体列表含中文字体，且可按名称加载；未知名称返回 None
    #[test]
    fn lists_and_loads_chinese_fonts() {
        let families = chinese_families();
        assert!(!families.is_empty(), "本机应至少有一个可显示中文的字体");
        assert!(families.windows(2).all(|w| w[0] < w[1]), "已排序去重");
        assert!(families.iter().all(|n| !n.starts_with('.')));
        let data = load_family(&families[0]).expect("列表中的字体应可加载");
        assert!(!(*data.font).as_ref().is_empty());
        assert!(load_family("不存在的字体XYZ").is_none());
    }
}
