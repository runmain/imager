//! UI E2E：通过 egui_kittest 无头驱动真实 App（点击、输入），以 SQLite 中的结果或渲染结果判定。

// 共享的 mock 服务只在 api_mock 中使用，这里只用 tiny_png
#[allow(dead_code)]
mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::{By, NodeT, Queryable};
use imageclient::app::App;
use imageclient::db::{AssistantResult, Db, ImageRecord, ProviderKind, Role, SentRequest, Status};

fn harness(db_path: &std::path::Path) -> Harness<'static, App> {
    let path = db_path.to_path_buf();
    Harness::builder().with_size([1280.0, 820.0]).with_theme(egui::Theme::Light).build_eframe(move |cc| {
        // eframe 原生启动时注册系统字体回退；kittest 不走原生启动，这里按同样方式注册
        cc.egui_ctx.add_font_provider(std::sync::Arc::new(egui_system_font_provider::SystemFontProvider::new()));
        let (db, backup) = Db::open(&path).unwrap();
        App::new(cc.egui_ctx.clone(), db, backup)
    })
}

fn provider_names(db_path: &std::path::Path) -> Vec<String> {
    let (db, _) = Db::open(db_path).unwrap();
    db.list_providers().unwrap().into_iter().map(|p| p.name).collect()
}

/// 预置：一个供应商、一个话题、一轮成功生成（图片已落盘）。
fn seed_success(db_path: &Path, image_dir: &Path) -> PathBuf {
    let (db, _) = Db::open(db_path).unwrap();
    let pid = db.add_provider("网关", "http://127.0.0.1:9", "sk-test-SECRET-123", ProviderKind::OpenAI).unwrap();
    db.set_provider_models(pid, &["gpt-image-1".to_string()]).unwrap();
    let topic = db.create_topic().unwrap();
    let sent = SentRequest {
        provider_id: pid,
        model_id: "gpt-image-1".into(),
        size: "1024x1024".into(),
        request_json: "{}".into(),
    };
    let user = db.add_user_message(topic.id, "一只猫", &sent).unwrap();
    let assistant = db.add_pending_assistant(topic.id, user).unwrap();
    std::fs::create_dir_all(image_dir).unwrap();
    let path = image_dir.join("img_test_0.png");
    std::fs::write(&path, common::tiny_png()).unwrap();
    db.finish_assistant(
        assistant,
        &AssistantResult {
            status: Status::Success,
            http_status: Some(200),
            response_body: Some(r#"{"created":1700000000,"data":[{"b64_json":"<base64 70 bytes -> x>"}]}"#.into()),
            error: None,
            failures: Vec::new(),
            images: vec![ImageRecord {
                local_path: path.display().to_string(),
                format: "png".into(),
                width: Some(2),
                height: Some(3),
                source_field: "b64_json".into(),
                origin: None,
            }],
        },
    )
    .unwrap();
    path
}

/// 后台任务（纹理解码、剪贴板图片）在线程中完成，逐帧推进直到条件满足。
fn step_until(h: &mut Harness<'static, App>, what: &str, mut done: impl FnMut(&Harness<'static, App>) -> bool) {
    for _ in 0..200 {
        h.step();
        if done(h) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("等待超时：{what}");
}

/// 采样点必须是不透明的浅色（kittest 以透明色清屏，未填充背景的面板会是透明 / 深色）。
fn assert_light(img: &image::RgbaImage, x: u32, y: u32, area: &str) {
    let p = img.get_pixel(x, y).0;
    assert!(p[3] == 255 && p[0] > 200 && p[1] > 200 && p[2] > 200, "{area} 背景应为浅色，实际像素 {p:?}");
}

// AC-021：聊天区、输入区、供应商列表背景为浅色
#[test]
fn backgrounds_are_light() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.db");
    seed_success(&db_path, &dir.path().join("images"));
    let mut h = harness(&db_path);
    h.run();
    let img = h.render().unwrap();
    let (w, hgt) = (img.width(), img.height());
    assert_light(&img, w - 40, hgt / 2, "聊天消息区");
    assert_light(&img, w - 40, hgt - 10, "输入区");

    h.get_by_label("设置").click();
    h.run();
    h.get_by_label("供应商").click();
    h.run();
    let img = h.render().unwrap();
    // 设置导航 64 + 设置分类 180 之后是供应商列表（宽 260）
    assert_light(&img, (64 + 180 + 130) * img.width() / 1280, img.height() - 40, "供应商列表");
}

/// 是否存在内容包含 `needle` 的文本框（原始结果以只读文本框展示）。
fn has_text_value(h: &Harness<'static, App>, needle: &str) -> bool {
    h.query_all(By::new().predicate(|n| n.value().is_some_and(|v| v.contains(needle)))).next().is_some()
}

fn is_selected(h: &Harness<'static, App>, label: &str) -> bool {
    h.get_by_label(label).accesskit_node().data().toggled() == Some(egui::accesskit::Toggled::True)
}

// AC-022：原始结果开关
#[test]
fn raw_result_toggle() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.db");
    seed_success(&db_path, &dir.path().join("images"));
    let mut h = harness(&db_path);
    h.run();
    assert!(!has_text_value(&h, "\"created\""), "默认收起");
    assert!(!is_selected(&h, "原始结果"));
    h.get_by_label("原始结果").click();
    h.run();
    assert!(is_selected(&h, "原始结果"));
    h.get_by_label_contains("HTTP 200");
    assert!(has_text_value(&h, "\"created\""), "展开后显示响应体");
    h.get_by_label("原始结果").click();
    h.run();
    assert!(!has_text_value(&h, "\"created\""), "再次点击收起");
    assert!(!is_selected(&h, "原始结果"));
}

// AC-023 / BN-012：应用内预览（双击缩略图或“打开图片”，窗口居中、不放大）
#[test]
fn builtin_image_viewer() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.db");
    seed_success(&db_path, &dir.path().join("images"));
    let mut h = harness(&db_path);
    step_until(&mut h, "缩略图加载", |h| h.query_by_label("缩略图").is_some());
    h.get_by_label("缩略图").click();
    step_until(&mut h, "单击缩略图打开预览", |h| h.query_by_label("预览图").is_some());
    h.get_by_label("2x3");
    let shown = h.get_by_label("预览图").rect();
    assert!((shown.width() - 2.0).abs() < 0.5 && (shown.height() - 3.0).abs() < 0.5, "小图保持原尺寸：{shown:?}");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.query_by_label("预览图").is_none(), "Esc 关闭预览");

    h.get_by_label("打开图片").click();
    step_until(&mut h, "按钮打开预览", |h| h.query_by_label("预览图").is_some());
    h.get_by_label("关闭").click();
    h.run();
    assert!(h.query_by_label("预览图").is_none(), "关闭按钮关闭预览");
}

#[test]
fn preview_fits_window() {
    use imageclient::ui::viewer::fit_size;
    let max = egui::vec2(800.0, 600.0);
    assert_eq!(fit_size(egui::vec2(1600.0, 900.0), max), egui::vec2(800.0, 450.0));
    assert_eq!(fit_size(egui::vec2(1000.0, 2000.0), max), egui::vec2(300.0, 600.0));
    assert_eq!(fit_size(egui::vec2(200.0, 100.0), max), egui::vec2(200.0, 100.0));
}

/// 带 40px 红色边框的蓝色测试图：截图中红色像素的范围即预览图实际显示的范围。
fn bordered_png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        if x < 40 || y < 40 || x >= w - 40 || y >= h - 40 {
            image::Rgb([230, 20, 20])
        } else {
            image::Rgb([40, 90, 220])
        }
    });
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// 截图中鲜红像素的包围盒（像素中心坐标）；遮罩下的缩略图颜色偏暗，不计入。
fn red_bounds(img: &image::RgbaImage) -> Option<egui::Rect> {
    let mut r = egui::Rect::NOTHING;
    for (x, y, p) in img.enumerate_pixels() {
        let [red, green, blue, _] = p.0;
        if red > 200 && green < 60 && blue < 60 {
            r.extend_with(egui::pos2(x as f32 + 0.5, y as f32 + 0.5));
        }
    }
    r.is_positive().then_some(r)
}

// image-client-mvp AC-023 / AC-028 / BN-014：打开预览即完整显示整张图并水平居中，无需拖动
// （宽高不为 1000 的大图曾整体偏移并被裁切）
#[test]
fn preview_initially_shows_whole_image_centered() {
    use imageclient::ui::viewer::fit_size;
    for (w, hgt) in [(1536u32, 1024u32), (1024, 1536)] {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("t.db");
        let path = seed_success(&db_path, &dir.path().join("images"));
        std::fs::write(&path, bordered_png(w, hgt)).unwrap();
        let mut h = harness(&db_path);
        step_until(&mut h, "缩略图加载", |h| h.query_by_label("缩略图").is_some());
        h.get_by_label("缩略图").click();
        step_until(&mut h, "预览图加载", |h| h.query_by_label("预览图").is_some());
        h.run();
        let img = h.render().unwrap();
        let shown = red_bounds(&img).expect("预览中应能看到图片的红色边框");
        // 包围盒取像素中心，四周各差半个像素
        let size = shown.size() + egui::Vec2::splat(1.0);
        let expect = fit_size(egui::vec2(w as f32, hgt as f32), egui::vec2(1280.0, 820.0) * 0.8);
        assert!(
            (size - expect).abs().max_elem() <= 2.5,
            "{w}x{hgt}：实际显示 {size:?}，应完整显示为 {expect:?}"
        );
        let mid = img.width() as f32 / 2.0;
        assert!(
            (shown.center().x - mid).abs() <= 2.5,
            "{w}x{hgt}：图片中心 x={} 应在窗口中线 {mid}",
            shown.center().x
        );
    }
}

// AC-024：复制图片写入剪贴板并提示
#[test]
fn copy_image_to_clipboard() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.db");
    seed_success(&db_path, &dir.path().join("images"));
    let mut h = harness(&db_path);
    h.run();
    h.get_by_label("复制图片").click();
    let mut copied = None;
    step_until(&mut h, "复制图片命令", |h| {
        copied = h.output().platform_output.commands.iter().find_map(|c| match c {
            egui::OutputCommand::CopyImage(img) => Some(img.size),
            _ => None,
        });
        copied.is_some()
    });
    assert_eq!(copied, Some([2, 3]), "剪贴板写入原图尺寸");
    h.run();
    h.get_by_label("已复制图片");
}

// AC-001 / v0.8：添加供应商必须填写名称
#[test]
fn add_provider_requires_name() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.db");
    let mut h = harness(&db_path);
    h.run();

    h.get_by_label("设置").click();
    h.run();
    h.get_by_label("供应商").click();
    h.run();
    h.get_by_label("+ 添加供应商").click();
    h.run();

    // 名称为空：提示可见，“添加”不可用，点击也不会落库
    h.get_by_label("请填写供应商名称");
    let add = h.get_by_label("添加");
    assert!(add.accesskit_node().is_disabled(), "添加按钮在名称为空时应禁用");
    add.click();
    h.run();
    assert!(provider_names(&db_path).is_empty(), "名称为空不应创建供应商");

    // 只输入空白同样不可提交
    h.get_by_label("名称 *").focus();
    h.run();
    h.get_by_label("名称 *").type_text("   ");
    h.run();
    assert!(h.get_by_label("添加").accesskit_node().is_disabled());

    // 填写名称后可添加，名称去掉首尾空白
    h.get_by_label("名称 *").type_text("我的网关 ");
    h.run();
    let add = h.get_by_label("添加");
    assert!(!add.accesskit_node().is_disabled());
    add.click();
    h.run();
    assert_eq!(provider_names(&db_path), vec!["我的网关".to_string()]);
    assert!(h.query_by_label("名称 *").is_none(), "添加后窗口关闭");

    let _ = h.try_snapshot("add_provider_done");
}

fn body_size(h: &Harness<'static, App>) -> f32 {
    h.ctx.global_style().text_styles[&egui::TextStyle::Body].size
}

/// 打开 设置 → 偏好设置。
fn open_preferences(h: &mut Harness<'static, App>) {
    h.get_by_label("设置").click();
    h.run();
    h.get_by_label("偏好设置").click();
    h.run();
}

// AC-026：界面字号即时生效并持久化；启动时按偏好应用
#[test]
fn font_size_applies() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.db");
    {
        let (db, _) = Db::open(&db_path).unwrap();
        let mut s = db.load_settings().unwrap();
        s.ui_font_size = 16;
        db.save_settings(&s).unwrap();
    }
    let mut h = harness(&db_path);
    h.run();
    assert_eq!(body_size(&h), 16.0, "启动时应用已保存的字号");

    open_preferences(&mut h);
    h.get_by_label("界面字号").set_value_accesskit(17.0);
    h.run();
    assert_eq!(body_size(&h), 17.0, "拖动后立即生效");
    let (db, _) = Db::open(&db_path).unwrap();
    assert_eq!(db.load_settings().unwrap().ui_font_size, 17, "写入偏好");
}

// AC-025 / BN-013：已保存的字体不存在时回退系统字体并提示，偏好值不被改写
#[test]
fn ui_font_fallback_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.db");
    {
        let (db, _) = Db::open(&db_path).unwrap();
        let mut s = db.load_settings().unwrap();
        s.ui_font = Some("不存在的字体XYZ".into());
        db.save_settings(&s).unwrap();
    }
    let mut h = harness(&db_path);
    step_until(&mut h, "字体不可用提示", |h| {
        h.query_by_label_contains("界面字体「不存在的字体XYZ」不可用").is_some()
    });
    let (db, _) = Db::open(&db_path).unwrap();
    assert_eq!(db.load_settings().unwrap().ui_font.as_deref(), Some("不存在的字体XYZ"));

    // 偏好设置中可见字体下拉，当前值显示保存的名称
    open_preferences(&mut h);
    h.get_by_label("界面字体");
}

// AC-027：尺寸为分段控件，点击即切换
#[test]
fn segmented_size_picker() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.db");
    seed_success(&db_path, &dir.path().join("images"));
    let mut h = harness(&db_path);
    h.run();
    assert!(is_selected(&h, "1024x1024"));
    assert!(!is_selected(&h, "2048x2048"));
    h.get_by_label("2048x2048").click();
    h.run();
    assert!(is_selected(&h, "2048x2048"));
    assert!(!is_selected(&h, "1024x1024"));
}
#[test]
fn custom_size_picker() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.db");
    seed_success(&db_path, &dir.path().join("images"));
    let mut h = harness(&db_path);
    h.run();

    // 默认是 1024x1024
    assert!(is_selected(&h, "1024x1024"));

    // 点击 2048x2048 分段，分段选中
    h.get_by_label("2048x2048").click();
    h.run();
    assert!(is_selected(&h, "2048x2048"));

    // 修改自定义宽高并提交
    h.state_mut().set_custom_size_for_test("128", "128");
    h.run();

    assert_eq!(h.state().chat_size_str(), "128x128");
    // 不在预设中，分段控件全未选中
    assert!(!is_selected(&h, "1024x1024"));
    assert!(!is_selected(&h, "2048x2048"));
    assert!(!is_selected(&h, "4096x4096"));

    // 再次点击预设 1024x1024，同步更新自定义输入框
    h.get_by_label("1024x1024").click();
    h.run();
    assert!(is_selected(&h, "1024x1024"));
    assert_eq!(h.state().custom_size_pair(), ("1024", "1024"));
}

// ---------- assistant-chat ----------

/// 预置一个指向 mock 网关的供应商（含一个保留模型），返回供应商 id。
fn seed_chat_provider(db_path: &Path, base: &str, kind: ProviderKind) -> i64 {
    let (db, _) = Db::open(db_path).unwrap();
    let pid = db.add_provider("网关", base, "sk-test-SECRET-123", kind).unwrap();
    db.set_provider_models(pid, &["chat-model".to_string()]).unwrap();
    pid
}

#[test]
fn chat_models_exclude_image_video() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.db");
    {
        let (db, _) = Db::open(&db_path).unwrap();
        let pid = db.add_provider("网关", "http://127.0.0.1:9", "sk-test", ProviderKind::OpenAI).unwrap();
        db.set_provider_models(pid, &["gpt-image-1".to_string(), "Sora-Video-2".to_string(), "gpt-4o".to_string()])
            .unwrap();
    }
    let mut h = harness(&db_path);
    h.run();
    h.get_by_label("聊天").click();
    h.run();
    // 默认选中首个聊天模型，而非排在前面的生图 / 视频模型
    h.get_by_value("gpt-4o").click();
    h.run();
    assert!(h.query_by_label("网关 / gpt-4o").is_some());
    assert!(h.query_by_label("网关 / gpt-image-1").is_none());
    assert!(h.query_by_label("网关 / Sora-Video-2").is_none());
}

#[test]
fn chat_conversation_gets_generated_title() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.db");
    let chat = common::MockServer::start(vec![(
        "/messages",
        common::Reply::sse(&[
            ("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"你好"}}"#),
            ("message_stop", r#"{"type":"message_stop"}"#),
        ]),
    )]);
    let titler = common::MockServer::start(vec![("/responses", common::Reply::json(200, r#"{"output_text":"问候对话"}"#))]);
    seed_chat_provider(&db_path, &chat.base, ProviderKind::Anthropic);
    {
        let (db, _) = Db::open(&db_path).unwrap();
        let tp = db.add_provider("标题网关", &titler.base, "sk-title", ProviderKind::OpenAI).unwrap();
        db.set_provider_models(tp, &["title-model".to_string()]).unwrap();
        let mut s = db.load_settings().unwrap();
        s.title_provider_id = Some(tp);
        s.title_model_id = "title-model".into();
        db.save_settings(&s).unwrap();
    }
    let mut h = harness(&db_path);
    h.run();
    open_default_assistant(&mut h);
    h.get_by_label("+ 新建").click();
    h.run();
    chat_send(&mut h, "你好呀");

    let titled = |_: &Harness<'static, App>| {
        let (db, _) = Db::open(&db_path).unwrap();
        let def = db.list_assistants().unwrap().remove(0);
        db.list_conversations(def.id).unwrap().first().is_some_and(|c| c.title == "问候对话")
    };
    step_until(&mut h, "对话标题生成", titled);
    let req = titler.recorded();
    assert_eq!(req.len(), 1);
    assert_eq!(req[0].path, "/responses");
    assert!(req[0].body.contains("title-model") && req[0].body.contains("你好呀"));
    h.run();
    // 侧栏话题行与顶栏标题同时显示新标题
    assert_eq!(h.query_all_by_label("问候对话").count(), 2);
}

/// 在聊天输入框输入并点击发送，等待该对话生成结束。
fn chat_send(h: &mut Harness<'static, App>, text: &str) {
    h.get_by_label("聊天输入").focus();
    h.run();
    h.get_by_label("聊天输入").type_text(text);
    h.run();
    h.get_by_label("发送").click();
    // 生成中有转圈与流式增量持续重绘，只能逐帧推进
    h.step();
    step_until(h, "聊天回复完成", |h| !h.state().is_chatting());
    h.run();
}

/// 打开聊天页并进入 default 助手的话题列表。
fn open_default_assistant(h: &mut Harness<'static, App>) {
    h.get_by_label("聊天").click();
    h.run();
    h.get_by_label("助手").click();
    h.run();
    h.get_by_label("default").click();
    h.run();
}

// assistant-chat AC-005, AC-006, AC-008, AC-009, AC-011：两轮对话，第二轮请求携带首轮历史
#[test]
fn chat_send_multi_turn() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.db");
    let gw = common::MockServer::start(vec![(
        "/responses",
        common::Reply::sse(&[
            ("response.output_text.delta", r#"{"type":"response.output_text.delta","delta":"回"}"#),
            ("response.output_text.delta", r#"{"type":"response.output_text.delta","delta":"复"}"#),
            ("response.completed", r#"{"type":"response.completed","response":{}}"#),
        ]),
    )]);
    seed_chat_provider(&db_path, &gw.base, ProviderKind::OpenAI);
    {
        let (db, _) = Db::open(&db_path).unwrap();
        let def = db.list_assistants().unwrap().remove(0);
        db.update_assistant(&imageclient::db::Assistant { system_prompt: "你是助手".into(), ..def }).unwrap();
    }
    let mut h = harness(&db_path);
    h.run();
    open_default_assistant(&mut h);
    // 点击助手后切到话题页签
    assert!(is_selected(&h, "话题"));
    h.get_by_label("+ 新建").click();
    h.run();

    chat_send(&mut h, "第一问");
    chat_send(&mut h, "第二问");

    let (db, _) = Db::open(&db_path).unwrap();
    let def = db.list_assistants().unwrap().remove(0);
    let convs = db.list_conversations(def.id).unwrap();
    assert_eq!(convs.len(), 1, "新建对话归属 default 助手");
    let msgs = db.list_chat_messages(convs[0].id).unwrap();
    let got: Vec<(Role, &str, Status)> = msgs.iter().map(|m| (m.role, m.content.as_str(), m.status)).collect();
    assert_eq!(
        got,
        vec![
            (Role::User, "第一问", Status::Success),
            (Role::Assistant, "回复", Status::Success),
            (Role::User, "第二问", Status::Success),
            (Role::Assistant, "回复", Status::Success),
        ]
    );
    // AC-011：首轮后标题不再是默认值（未配置标题供应商时回退为首句）
    assert_eq!(db.get_conversation(convs[0].id).unwrap().unwrap().title, "第一问");

    let chats: Vec<_> = gw.recorded().into_iter().filter(|r| r.path == "/responses").collect();
    assert_eq!(chats.len(), 2);
    let second: serde_json::Value = serde_json::from_str(&chats[1].body).unwrap();
    assert_eq!(second["instructions"], "你是助手");
    assert_eq!(second["model"], "chat-model");
    assert_eq!(
        second["input"],
        serde_json::json!([
            {"role":"user","content":[{"type":"input_text","text":"第一问"}]},
            {"role":"assistant","content":[{"type":"output_text","text":"回复"}]},
            {"role":"user","content":[{"type":"input_text","text":"第二问"}]}
        ])
    );
    // BN-004：保存的请求体不含 Key
    assert!(msgs.iter().all(|m| !m.request_json.clone().unwrap_or_default().contains("SECRET")));
}

// assistant-chat AC-010：回复按 Markdown 渲染
#[test]
fn chat_reply_renders_markdown() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.db");
    let gw = common::MockServer::start(vec![(
        "/messages",
        common::Reply::sse(&[
            ("content_block_delta", r##"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"# 大标题\n\n- 条目一\n"}}"##),
            ("message_stop", r#"{"type":"message_stop"}"#),
        ]),
    )]);
    seed_chat_provider(&db_path, &gw.base, ProviderKind::Anthropic);
    let mut h = harness(&db_path);
    h.run();
    open_default_assistant(&mut h);
    chat_send(&mut h, "给我标题");
    h.get_by_label("大标题");
    h.get_by_label("条目一");
    assert!(h.query_by_label_contains("# 大标题").is_none(), "Markdown 标记不应原样显示");
}

/// 预置一轮已完成的聊天（用户消息 + 成功的助手回复）；打开 default 助手即显示该对话。
fn seed_chat_reply(db_path: &Path, user: &str, reply: &str) {
    let (db, _) = Db::open(db_path).unwrap();
    let pid = db.add_provider("网关", "http://127.0.0.1:9", "sk-test", ProviderKind::OpenAI).unwrap();
    db.set_provider_models(pid, &["chat-model".to_string()]).unwrap();
    let def = db.list_assistants().unwrap().remove(0);
    let conv = db.create_conversation(def.id).unwrap();
    db.add_chat_user(conv.id, user, pid, "chat-model", "{}").unwrap();
    let reply_id = db.add_chat_pending(conv.id).unwrap();
    db.finish_chat(reply_id, Status::Success, reply, Some(200), None).unwrap();
}

// assistant-chat AC-016 / BN-011：回复正文与用户气泡同字体、同字号、同字距（同一段文字排版宽度相同），
// 单行段落行高为正文字号的 1.6 倍
#[test]
fn chat_reply_body_matches_ui_font_size() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.db");
    let text = "排版测试：中文与 English 混排";
    seed_chat_reply(&db_path, text, text);
    let mut h = harness(&db_path);
    h.run();
    open_default_assistant(&mut h);
    let mut rects: Vec<egui::Rect> = h.query_all_by_label(text).map(|n| n.rect()).collect();
    assert_eq!(rects.len(), 2, "用户气泡与回复各一处");
    rects.sort_by(|a, b| a.min.y.total_cmp(&b.min.y));
    let (user, reply) = (rects[0], rects[1]);
    assert!(
        (reply.width() - user.width()).abs() <= 0.5,
        "回复宽 {} 应与用户气泡宽 {} 相同",
        reply.width(),
        user.width()
    );
    assert_eq!(reply.height(), (body_size(&h) * 1.6).round(), "单行段落行高");
}

// assistant-chat AC-016 / BN-010：可用宽度足够时表格单元格不折行（曾因列宽取表头宽度而中文逐字折行）
#[test]
fn chat_reply_table_cells_do_not_wrap() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.db");
    seed_chat_reply(&db_path, "表格", "单行段落\n\n| 特性 | 说明 |\n|---|---|\n| 所有权 | 每个值唯一所有者 |\n");
    let mut h = harness(&db_path);
    h.run();
    open_default_assistant(&mut h);
    let line = h.get_by_label("单行段落").rect().height();
    for cell in ["特性", "说明", "所有权", "每个值唯一所有者"] {
        let r = h.get_by_label(cell).rect();
        assert!(r.height() <= line + 0.5, "单元格「{cell}」折行：高 {}，单行 {line}", r.height());
    }
    assert_eq!(h.get_by_label("特性").rect().min.x, h.get_by_label("所有权").rect().min.x, "同列左对齐");
}

// assistant-chat BN-007：生成中不可再次发送
#[test]
fn chat_send_blocked_while_generating() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.db");
    // 端口 9 不可达，但连接失败前状态已置为生成中；直接检查发送阻断原因
    seed_chat_provider(&db_path, "http://127.0.0.1:9", ProviderKind::OpenAI);
    let mut h = harness(&db_path);
    h.run();
    open_default_assistant(&mut h);
    assert_eq!(h.state().chat_send_blocker_for_test(), Some("请输入内容"));
    h.state_mut().mark_chatting_for_test();
    h.get_by_label("聊天输入").focus();
    h.run();
    h.get_by_label("聊天输入").type_text("x");
    h.run();
    assert_eq!(h.state().chat_send_blocker_for_test(), Some("当前对话正在生成中"));
    assert!(h.get_by_label("发送").accesskit_node().is_disabled());
}

// assistant-chat AC-002（UI）：添加供应商时选择 Claude 类型
#[test]
fn provider_kind_selectable() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.db");
    let mut h = harness(&db_path);
    h.run();
    h.get_by_label("设置").click();
    h.run();
    h.get_by_label("供应商").click();
    h.run();
    h.get_by_label("+ 添加供应商").click();
    h.run();
    h.get_by_label("名称 *").focus();
    h.run();
    h.get_by_label("名称 *").type_text("C");
    h.run();
    h.get_by_label("类型").click();
    h.run();
    h.get_by_label("Claude").click();
    h.run();
    h.get_by_label("添加").click();
    h.run();
    let (db, _) = Db::open(&db_path).unwrap();
    assert_eq!(db.list_providers().unwrap()[0].kind, ProviderKind::Anthropic);
}

// assistant-chat AC-004（UI）, BN-001：设置中添加助手；default 无删除按钮
#[test]
fn settings_add_assistant() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.db");
    let mut h = harness(&db_path);
    h.run();
    h.get_by_label("设置").click();
    h.run();
    h.get_by_label("助手").click();
    h.run();
    h.get_by_label("default").click();
    h.run();
    assert!(h.query_by_label("删除助手").is_none(), "default 不可删除");
    h.get_by_label("+ 添加助手").click();
    h.run();
    let (db, _) = Db::open(&db_path).unwrap();
    let list = db.list_assistants().unwrap();
    assert_eq!(list.len(), 2);
    assert!(h.query_by_label("删除助手").is_some(), "新助手可删除");
}
