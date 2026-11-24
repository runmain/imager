//! 话题标题：提示词组装、结果清洗、兜底标题、自动生成判定。

pub const MAX_TITLE_CHARS: usize = 10;
pub const DEFAULT_TOPIC_TITLE: &str = "新话题";
pub const DEFAULT_CONVERSATION_TITLE: &str = "新对话";
pub const DEFAULT_TITLE_PROMPT: &str = "根据用户提供的内容，帮我生成一个标题。\n要求：\n1. 10个字以内；\n2. 标题要体现用户的真实意图（想做什么、用在哪里），不要写成“生成xxx图片”“xxx风格”这类泛泛的描述；\n3. 只输出标题本身，不要标点、引号和解释。\n用户内容：{prompt}";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleSource {
    Default,
    Fallback,
    Generated,
    Manual,
}

impl TitleSource {
    pub fn as_str(self) -> &'static str {
        match self {
            TitleSource::Default => "default",
            TitleSource::Fallback => "fallback",
            TitleSource::Generated => "generated",
            TitleSource::Manual => "manual",
        }
    }

    pub fn parse(s: &str) -> TitleSource {
        match s {
            "fallback" => TitleSource::Fallback,
            "generated" => TitleSource::Generated,
            "manual" => TitleSource::Manual,
            _ => TitleSource::Default,
        }
    }
}

/// 标题调用所需配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleTarget {
    pub provider_id: i64,
    pub model: String,
}

/// 校验标题配置：未选供应商、未指定模型、供应商禁用/不存在时返回原因。
/// `provider_enabled` 为 `None` 表示该供应商已不存在。
pub fn resolve_target(
    provider_id: Option<i64>,
    model: &str,
    provider_enabled: Option<bool>,
) -> Result<TitleTarget, String> {
    let Some(provider_id) = provider_id else {
        return Err("未配置标题供应商".into());
    };
    let model = model.trim();
    if model.is_empty() {
        return Err("未指定标题模型".into());
    }
    match provider_enabled {
        None => Err("标题供应商不存在".into()),
        Some(false) => Err("标题供应商已禁用".into()),
        Some(true) => Ok(TitleTarget { provider_id, model: model.to_string() }),
    }
}

/// 用首条用户消息替换 `{prompt}`；模板中没有占位符时追加到末尾。
pub fn build_input(template: &str, prompt: &str) -> String {
    if template.contains("{prompt}") {
        template.replace("{prompt}", prompt)
    } else {
        format!("{template}\n{prompt}")
    }
}

const TITLE_TRIM: &[char] = &[
    '"', '\'', '“', '”', '‘', '’', '「', '」', '『', '』', '《', '》', '〈', '〉', '【', '】',
    '。', '，', '.', ',', '！', '!', '？', '?', '；', ';', '：', ':', '`', '*', '#',
];

/// 清洗标题接口返回：取第一行有效内容，去掉“标题：”前缀、引号书名号和首尾标点，截断到 10 个字符。
pub fn clean_title(raw: &str) -> Option<String> {
    raw.lines().find_map(|line| {
        let mut s = line.trim();
        for prefix in ["标题：", "标题:", "Title:", "title:"] {
            if let Some(rest) = s.strip_prefix(prefix) {
                s = rest;
            }
        }
        let s = s.trim_matches(|c: char| c.is_whitespace() || TITLE_TRIM.contains(&c));
        if s.is_empty() {
            return None;
        }
        Some(s.chars().take(MAX_TITLE_CHARS).collect::<String>().trim_end().to_string())
    })
}

/// 兜底标题：首条消息压缩空白后取前 10 个字符；为空时用默认标题。
pub fn fallback_title(prompt: &str) -> String {
    let collapsed = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    let title: String = collapsed.chars().take(MAX_TITLE_CHARS).collect();
    let title = title.trim_end();
    if title.is_empty() { DEFAULT_TOPIC_TITLE.to_string() } else { title.to_string() }
}

/// 只有从未生成过标题的话题才自动生成（只触发一次）。
pub fn should_auto_generate(source: TitleSource) -> bool {
    source == TitleSource::Default
}

/// 标题接口结果回来时是否应用：手动重新生成（force）总是应用；自动生成时不覆盖手动标题。
pub fn should_apply(current: TitleSource, force: bool) -> bool {
    force || current != TitleSource::Manual
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_roundtrip() {
        for s in [TitleSource::Default, TitleSource::Fallback, TitleSource::Generated, TitleSource::Manual] {
            assert_eq!(TitleSource::parse(s.as_str()), s);
        }
        assert_eq!(TitleSource::parse("???"), TitleSource::Default);
    }

    // AC-015, AC-017
    #[test]
    fn target_requires_provider_model_and_enabled() {
        assert_eq!(
            resolve_target(Some(3), " gpt-4o-mini ", Some(true)),
            Ok(TitleTarget { provider_id: 3, model: "gpt-4o-mini".into() })
        );
        assert!(resolve_target(None, "m", None).unwrap_err().contains("未配置标题供应商"));
        assert!(resolve_target(Some(3), "  ", Some(true)).unwrap_err().contains("未指定标题模型"));
        assert!(resolve_target(Some(3), "m", Some(false)).unwrap_err().contains("禁用"));
        assert!(resolve_target(Some(3), "m", None).unwrap_err().contains("不存在"));
    }

    #[test]
    fn build_input_replaces_or_appends() {
        assert_eq!(build_input("标题：{prompt}。", "猫"), "标题：猫。");
        assert_eq!(build_input("生成标题", "猫"), "生成标题\n猫");
    }

    // BN-009
    #[test]
    fn clean_title_strips_and_truncates() {
        assert_eq!(clean_title("  「橘猫晒太阳」\n").as_deref(), Some("橘猫晒太阳"));
        assert_eq!(clean_title("\"Cat\"").as_deref(), Some("Cat"));
        assert_eq!(clean_title("标题：《一二三》").as_deref(), Some("一二三"));
        let long = clean_title("一二三四五六七八九十十一十二").unwrap();
        assert_eq!(long.chars().count(), MAX_TITLE_CHARS);
        assert_eq!(long, "一二三四五六七八九十");
        assert_eq!(clean_title(" \n\"\" "), None);
    }

    // AC-017
    #[test]
    fn fallback_uses_first_10_chars() {
        assert_eq!(fallback_title("  企业级 Web 系统高保真 UI  "), "企业级 Web 系统");
        assert_eq!(fallback_title("猫\n狗"), "猫 狗");
        assert_eq!(fallback_title("   "), DEFAULT_TOPIC_TITLE);
    }

    // BN-009
    #[test]
    fn auto_generate_only_once_and_never_over_manual() {
        assert!(should_auto_generate(TitleSource::Default));
        assert!(!should_auto_generate(TitleSource::Generated));
        assert!(!should_auto_generate(TitleSource::Fallback));
        assert!(!should_auto_generate(TitleSource::Manual));
        assert!(!should_apply(TitleSource::Manual, false));
        assert!(should_apply(TitleSource::Default, false));
        assert!(should_apply(TitleSource::Manual, true));
    }
}
