//! 助手回复的 Markdown 渲染：`pulldown-cmark` 解析为块结构，再用 egui 排版。
//! 解析结果按消息缓存（见 [`Cache`]），流式输出时内容变化才重新解析。
//! 版式：正文与界面正文同字体、同字号，行高 1.6 倍；块间距按正文字号的倍数计算（见 [`gap_before`]）。

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextFormat, text::LayoutJob};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use super::icons::{self, Icon};
use super::style;

/// 粗体字体族名；由 `fonts::install` 注册，无真实粗体时与常规字体相同。
pub const BOLD_FAMILY: &str = "md-bold";

/// 行高（字号的倍数）：正文 / 标题。
const BODY_LEADING: f32 = 1.6;
const HEADING_LEADING: f32 = 1.4;
/// 以下尺寸均为正文字号的倍数（em）。
/// 块间距：普通块之间、一二级标题上方、其余标题上方、标题下方、列表项内。
const BLOCK_GAP: f32 = 0.75;
const HEADING_GAP_MAJOR: f32 = 1.25;
const HEADING_GAP: f32 = 1.1;
const HEADING_GAP_BELOW: f32 = 0.5;
const ITEM_GAP: f32 = 0.3;
/// 行内代码：底色左右内边距、底色与相邻文字的间隔。
const CODE_PAD: f32 = 0.3;
const CODE_GAP: f32 = 0.15;
/// 表格：单元格左右 / 上下内边距、收窄时的最小列宽。
const CELL_PAD_X: f32 = 0.75;
const CELL_PAD_Y: f32 = 0.35;
const MIN_COLUMN: f32 = 4.0;

const INLINE_CODE_BG: Color32 = Color32::from_rgb(0xec, 0xef, 0xf3);
const INLINE_CODE_FG: Color32 = Color32::from_rgb(0x47, 0x55, 0x69);
const CODE_BLOCK_BG: Color32 = Color32::from_rgb(0xf8, 0xf9, 0xfb);
const CODE_HEADER_BG: Color32 = Color32::from_rgb(0xee, 0xf0, 0xf3);
const CODE_BLOCK_BORDER: Color32 = Color32::from_rgb(0xdd, 0xe1, 0xe6);
const QUOTE_BAR: Color32 = Color32::from_rgb(0xdd, 0xe1, 0xe6);
const TABLE_RADIUS: u8 = 8;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inline {
    pub text: String,
    pub strong: bool,
    pub emph: bool,
    pub strike: bool,
    pub code: bool,
    pub link: Option<Arc<str>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Para(Vec<Inline>),
    Heading(u8, Vec<Inline>),
    Code { lang: String, text: String },
    List { start: Option<u64>, items: Vec<Item> },
    Quote(Vec<Block>),
    Table { head: Vec<Vec<Inline>>, rows: Vec<Vec<Vec<Inline>>> },
    Rule,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

// ---------- 解析 ----------

pub fn parse(text: &str) -> Vec<Block> {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut it = Parser::new_ext(text, opts).peekable();
    parse_blocks(&mut it, |_| false)
}

type Events<'a> = std::iter::Peekable<Parser<'a>>;

fn is_block_start(e: &Event) -> bool {
    matches!(
        e,
        Event::Start(
            Tag::Paragraph
                | Tag::Heading { .. }
                | Tag::CodeBlock(_)
                | Tag::List(_)
                | Tag::BlockQuote(_)
                | Tag::Table(_)
                | Tag::HtmlBlock
        ) | Event::Rule
    )
}

/// 读取块，直到 `is_end` 命中的结束事件（消费掉）或事件流结束。
fn parse_blocks(it: &mut Events, is_end: fn(&TagEnd) -> bool) -> Vec<Block> {
    let mut blocks = Vec::new();
    while let Some(e) = it.peek() {
        if let Event::End(end) = e
            && is_end(end)
        {
            it.next();
            break;
        }
        // 紧凑列表项里的文字不在段落中：收集到下一个块或结束事件为止
        if !is_block_start(e) && !matches!(e, Event::Start(_) | Event::End(_)) || is_inline_start(e) {
            let inlines = parse_inlines(it, |e| is_block_start(e) || matches!(e, Event::End(t) if !is_inline_end(t)));
            if !inlines.is_empty() {
                blocks.push(Block::Para(inlines));
            }
            continue;
        }
        let Some(e) = it.next() else { break };
        match e {
            Event::Start(Tag::Paragraph) => {
                let inlines = parse_inlines(it, |e| matches!(e, Event::End(TagEnd::Paragraph)));
                it.next();
                blocks.push(Block::Para(inlines));
            }
            Event::Start(Tag::Heading { level, .. }) => {
                let inlines = parse_inlines(it, |e| matches!(e, Event::End(TagEnd::Heading(_))));
                it.next();
                blocks.push(Block::Heading(level as u8, inlines));
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => l.split_whitespace().next().unwrap_or("").to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                let mut text = String::new();
                for e in it.by_ref() {
                    match e {
                        Event::Text(t) => text.push_str(&t),
                        Event::End(TagEnd::CodeBlock) => break,
                        _ => {}
                    }
                }
                let text = text.trim_end_matches('\n').to_string();
                blocks.push(Block::Code { lang, text });
            }
            Event::Start(Tag::HtmlBlock) => {
                let mut text = String::new();
                for e in it.by_ref() {
                    match e {
                        Event::Html(t) | Event::Text(t) => text.push_str(&t),
                        Event::End(TagEnd::HtmlBlock) => break,
                        _ => {}
                    }
                }
                let text = text.trim_end().to_string();
                if !text.is_empty() {
                    blocks.push(Block::Para(vec![Inline { text, ..Default::default() }]));
                }
            }
            Event::Start(Tag::List(start)) => {
                let mut items = Vec::new();
                while let Some(e) = it.next() {
                    match e {
                        Event::Start(Tag::Item) => {
                            let task = match it.peek() {
                                Some(Event::TaskListMarker(done)) => {
                                    let done = *done;
                                    it.next();
                                    Some(done)
                                }
                                _ => None,
                            };
                            let blocks = parse_blocks(it, |t| matches!(t, TagEnd::Item));
                            items.push(Item { task, blocks });
                        }
                        Event::End(TagEnd::List(_)) => break,
                        _ => {}
                    }
                }
                blocks.push(Block::List { start, items });
            }
            Event::Start(Tag::BlockQuote(_)) => {
                blocks.push(Block::Quote(parse_blocks(it, |t| matches!(t, TagEnd::BlockQuote(_)))));
            }
            Event::Start(Tag::Table(_)) => {
                let (mut head, mut rows, mut row) = (Vec::new(), Vec::new(), Vec::new());
                while let Some(e) = it.next() {
                    match e {
                        Event::Start(Tag::TableCell) => {
                            row.push(parse_inlines(it, |e| matches!(e, Event::End(TagEnd::TableCell))));
                            it.next();
                        }
                        Event::End(TagEnd::TableHead) => head = std::mem::take(&mut row),
                        Event::End(TagEnd::TableRow) => rows.push(std::mem::take(&mut row)),
                        Event::End(TagEnd::Table) => break,
                        _ => {}
                    }
                }
                blocks.push(Block::Table { head, rows });
            }
            Event::Rule => blocks.push(Block::Rule),
            _ => {}
        }
    }
    blocks
}

fn is_inline_start(e: &Event) -> bool {
    matches!(
        e,
        Event::Start(Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. } | Tag::Image { .. })
    )
}

fn is_inline_end(t: &TagEnd) -> bool {
    matches!(t, TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link | TagEnd::Image)
}

/// 读取行内内容，遇到 `stop` 命中的事件时停下（不消费）。
fn parse_inlines(it: &mut Events, stop: impl Fn(&Event) -> bool) -> Vec<Inline> {
    let (mut strong, mut emph, mut strike) = (0u32, 0u32, 0u32);
    let mut link: Option<Arc<str>> = None;
    let mut out: Vec<Inline> = Vec::new();
    while let Some(e) = it.peek() {
        if stop(e) {
            break;
        }
        let Some(e) = it.next() else { break };
        let mut push = |text: &str, code: bool| {
            let style = Inline {
                text: String::new(),
                strong: strong > 0,
                emph: emph > 0,
                strike: strike > 0,
                code,
                link: link.clone(),
            };
            // 相邻同样式片段合并，减少排版段数
            match out.last_mut() {
                Some(last) if Inline { text: String::new(), ..last.clone() } == style => last.text.push_str(text),
                _ => out.push(Inline { text: text.to_string(), ..style }),
            }
        };
        match e {
            Event::Text(t) | Event::InlineHtml(t) | Event::Html(t) => push(&t, false),
            Event::Code(t) | Event::InlineMath(t) | Event::DisplayMath(t) => push(&t, true),
            // 聊天回复里的单换行按换行显示
            Event::SoftBreak | Event::HardBreak => push("\n", false),
            Event::FootnoteReference(t) => push(&format!("[{t}]"), false),
            Event::Start(Tag::Strong) => strong += 1,
            Event::End(TagEnd::Strong) => strong = strong.saturating_sub(1),
            Event::Start(Tag::Emphasis) => emph += 1,
            Event::End(TagEnd::Emphasis) => emph = emph.saturating_sub(1),
            Event::Start(Tag::Strikethrough) => strike += 1,
            Event::End(TagEnd::Strikethrough) => strike = strike.saturating_sub(1),
            Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => link = Some(dest_url.as_ref().into()),
            Event::End(TagEnd::Link | TagEnd::Image) => link = None,
            _ => {}
        }
    }
    out
}

// ---------- 缓存 ----------

/// 按消息 ID 缓存解析结果；内容哈希变化（流式增量）时重新解析。
#[derive(Default)]
pub struct Cache(HashMap<i64, (u64, Arc<Vec<Block>>)>);

impl Cache {
    pub fn get(&mut self, id: i64, text: &str) -> Arc<Vec<Block>> {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut h);
        let hash = h.finish();
        match self.0.get(&id) {
            Some((k, blocks)) if *k == hash => blocks.clone(),
            _ => {
                let blocks = Arc::new(parse(text));
                self.0.insert(id, (hash, blocks.clone()));
                blocks
            }
        }
    }
}

// ---------- 排版 ----------

#[derive(Clone, Copy)]
struct Ctx {
    /// 当前文字字号。
    size: f32,
    /// 正文字号：间距按它计算，不随标题放大。
    body: f32,
    /// 行高（字号的倍数）。
    leading: f32,
    color: Color32,
    bold: bool,
    /// 列表项内：块间距收紧为条目间距。
    compact: bool,
}

impl Ctx {
    fn line_height(self) -> f32 {
        (self.size * self.leading).round()
    }

    /// 正文字号的 `k` 倍，取整到点。
    fn em(self, k: f32) -> f32 {
        (self.body * k).round()
    }
}

fn bold_family(ui: &egui::Ui) -> FontFamily {
    let name: Arc<str> = BOLD_FAMILY.into();
    let registered = ui.ctx().fonts(|f| f.definitions().families.contains_key(&FontFamily::Name(name.clone())));
    if registered { FontFamily::Name(name) } else { FontFamily::Proportional }
}

/// 渲染一组块；返回是否点击了某个“复制代码”按钮（供调用方提示）。
pub fn show(ui: &mut egui::Ui, blocks: &[Block]) -> bool {
    // 取正文样式的字号；`text_style_height` 是字体行高，随字体不同会比字号大 15%–35%
    let body = egui::TextStyle::Body.resolve(ui.style()).size;
    let ctx = Ctx { size: body, body, leading: BODY_LEADING, color: style::TEXT, bold: false, compact: false };
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        render_blocks(ui, blocks, ctx)
    })
    .inner
}

fn render_blocks(ui: &mut egui::Ui, blocks: &[Block], ctx: Ctx) -> bool {
    let mut copied = false;
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            ui.add_space(gap_before(&blocks[i - 1], block, ctx.body, ctx.compact));
        }
        ui.push_id(i, |ui| copied |= render_block(ui, block, ctx));
    }
    copied
}

/// 相邻两块之间的竖直间距：标题上方宽、下方窄；列表项内收紧。
fn gap_before(prev: &Block, next: &Block, body: f32, compact: bool) -> f32 {
    let em = match (prev, next) {
        (_, Block::Heading(level, _)) if *level <= 2 => HEADING_GAP_MAJOR,
        (_, Block::Heading(..)) => HEADING_GAP,
        (Block::Heading(..), _) => HEADING_GAP_BELOW,
        _ if compact => ITEM_GAP,
        _ => BLOCK_GAP,
    };
    (body * em).round()
}

fn render_block(ui: &mut egui::Ui, block: &Block, ctx: Ctx) -> bool {
    match block {
        Block::Para(inlines) => paragraph(ui, inlines, ctx),
        Block::Heading(level, inlines) => {
            let size = ctx.body
                + match level {
                    1 => 7.0,
                    2 => 5.0,
                    3 => 3.0,
                    _ => 1.0,
                };
            paragraph(ui, inlines, Ctx { size, leading: HEADING_LEADING, color: style::TEXT, bold: true, ..ctx });
        }
        Block::Code { lang, text } => return code_block(ui, lang, text, ctx),
        Block::List { start, items } => return list(ui, *start, items, ctx),
        Block::Quote(blocks) => {
            let inner = egui::Frame::new()
                .inner_margin(egui::Margin { left: 14, ..Default::default() })
                .show(ui, |ui| render_blocks(ui, blocks, Ctx { color: style::MUTED, ..ctx }));
            let r = inner.response.rect;
            ui.painter().rect_filled(
                egui::Rect::from_min_size(r.min, egui::vec2(3.0, r.height())),
                CornerRadius::same(2),
                QUOTE_BAR,
            );
            return inner.inner;
        }
        Block::Table { head, rows } => table(ui, head, rows, ctx),
        Block::Rule => {
            ui.add_space(4.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, CornerRadius::ZERO, style::BORDER_LIGHT);
            ui.add_space(4.0);
        }
    }
    false
}

/// 链接所在的字符区间与目标地址。
type LinkSpan = (Range<usize>, Arc<str>);

/// 一段行内文字的排版任务，以及链接、行内代码各自所在的字符区间。
struct InlineJob {
    job: LayoutJob,
    links: Vec<LinkSpan>,
    codes: Vec<Range<usize>>,
}

/// 行内片段排版。行内代码前后用排版留白（不插入字符）为底色内边距与间隔留出位置，
/// 底色由 [`paragraph`] 按字形位置画成圆角块。
fn inline_job(ui: &egui::Ui, inlines: &[Inline], ctx: Ctx) -> InlineJob {
    let bold = bold_family(ui);
    let line_height = Some(ctx.line_height());
    let (pad, gap) = (ctx.em(CODE_PAD), ctx.em(CODE_GAP));
    let mut out = InlineJob { job: LayoutJob::default(), links: Vec::new(), codes: Vec::new() };
    let mut chars = 0;
    // 上一段是行内代码时，留给其右侧内边距与间隔的空白
    let mut pending = 0.0;
    for s in inlines {
        let family = if s.strong || ctx.bold { bold.clone() } else { FontFamily::Proportional };
        let mut f = TextFormat {
            font_id: FontId::new(ctx.size, family),
            color: ctx.color,
            line_height,
            italics: s.emph,
            ..Default::default()
        };
        if s.strike {
            f.strikethrough = Stroke::new(1.0, ctx.color);
        }
        let mut leading = std::mem::take(&mut pending);
        let n = s.text.chars().count();
        if s.code {
            f.font_id = FontId::new(ctx.size - 1.5, FontFamily::Monospace);
            // 行高不撑满整行，与正文垂直居中对齐
            f.line_height = None;
            f.valign = egui::Align::Center;
            f.color = INLINE_CODE_FG;
            leading += pad + if chars == 0 { 0.0 } else { gap };
            pending = pad + gap;
            out.codes.push(chars..chars + n);
        }
        if let Some(url) = &s.link {
            f.color = style::ACCENT;
            f.underline = Stroke::new(1.0, style::ACCENT.gamma_multiply(0.4));
            out.links.push((chars..chars + n, url.clone()));
        }
        chars += n;
        out.job.append(&s.text, leading, f);
    }
    if pending > 0.0 {
        // 以行内代码结尾：补一段空文字，为右侧底色留出宽度
        let f = TextFormat { font_id: FontId::new(ctx.size, FontFamily::Proportional), line_height, ..Default::default() };
        out.job.append("", pending, f);
    }
    out
}

fn paragraph(ui: &mut egui::Ui, inlines: &[Inline], ctx: Ctx) {
    let InlineJob { mut job, links, codes } = inline_job(ui, inlines, ctx);
    job.wrap.max_width = ui.available_width();
    let galley = ui.painter().layout_job(job);
    // 行内代码底色：先占位，等 Label 落位后再填入，保证画在文字下方
    let backdrop = (!codes.is_empty()).then(|| ui.painter().add(egui::Shape::Noop));
    let resp = ui.add(egui::Label::new(galley.clone()).selectable(true));
    if let Some(idx) = backdrop {
        let origin = resp.rect.min.to_vec2();
        let pills = code_rects(&galley, &codes, ctx.em(CODE_PAD))
            .into_iter()
            .map(|r| egui::Shape::rect_filled(r.translate(origin), CornerRadius::same(4), INLINE_CODE_BG))
            .collect();
        ui.painter().set(idx, egui::Shape::Vec(pills));
    }
    if links.is_empty() {
        return;
    }
    let Some(pos) = resp.hover_pos() else { return };
    let index = galley.cursor_from_pos(pos - resp.rect.min).index.0;
    if let Some((_, url)) = links.iter().find(|(r, _)| r.contains(&index)) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        if resp.clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url.as_ref()));
        }
    }
}

/// 行内代码底色矩形（相对排版原点）：每个代码区间在每行一段，左右各加 `pad`，高度取代码字形行高。
fn code_rects(galley: &egui::Galley, codes: &[Range<usize>], pad: f32) -> Vec<egui::Rect> {
    let mut out = Vec::new();
    let mut first = 0;
    for row in &galley.rows {
        for range in codes {
            let glyphs = row.glyphs.iter().enumerate().filter(|(i, _)| range.contains(&(first + i)));
            let rect = glyphs.fold(egui::Rect::NOTHING, |r, (_, g)| r.union(g.logical_rect()));
            if rect.is_positive() {
                let r = rect.translate(row.pos.to_vec2()).expand2(egui::vec2(pad, 1.0));
                // 折行后代码位于行首时左侧没有留白，底色不越过文字左边缘
                out.push(egui::Rect::from_min_max(egui::pos2(r.min.x.max(0.0), r.min.y), r.max));
            }
        }
        first += row.char_count_including_newline().0;
    }
    out
}

/// 序号 / 圆点与条目首行同字号、同行高排版，从而与首行文字垂直对齐。
fn list(ui: &mut egui::Ui, start: Option<u64>, items: &[Item], ctx: Ctx) -> bool {
    let mut copied = false;
    let marker_ctx = Ctx { color: if start.is_some() { ctx.color } else { style::MUTED }, ..ctx };
    let marker = |ui: &egui::Ui, text: String| {
        ui.painter().layout_job(inline_job(ui, &[Inline { text, ..Default::default() }], marker_ctx).job)
    };
    let marker_w = match start {
        Some(n) => marker(ui, format!("{}.", n + items.len().saturating_sub(1) as u64)).size().x + ctx.em(0.6),
        None => ctx.em(1.4),
    };
    let item_ctx = Ctx { compact: true, ..ctx };
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            ui.add_space(ctx.em(ITEM_GAP));
        }
        ui.push_id(i, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(marker_w, ctx.line_height()), egui::Sense::hover());
                match (item.task, start) {
                    (Some(done), _) => task_box(ui, rect, done),
                    (None, Some(n)) => {
                        let g = marker(ui, format!("{}.", n + i as u64));
                        ui.painter().galley(rect.min, g, ctx.color);
                    }
                    (None, None) => {
                        let c = egui::pos2(rect.left() + ctx.em(0.45), rect.center().y);
                        ui.painter().circle_filled(c, 2.5, style::MUTED);
                    }
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    copied |= render_blocks(ui, &item.blocks, item_ctx);
                });
            });
        });
    }
    copied
}

fn task_box(ui: &egui::Ui, rect: egui::Rect, done: bool) {
    let b = egui::Rect::from_center_size(egui::pos2(rect.left() + 7.0, rect.center().y), egui::vec2(13.0, 13.0));
    if done {
        ui.painter().rect_filled(b, CornerRadius::same(3), style::ACCENT);
        let s = Stroke::new(1.6, Color32::WHITE);
        let p = |x: f32, y: f32| egui::pos2(b.left() + b.width() * x, b.top() + b.height() * y);
        ui.painter().line(vec![p(0.25, 0.52), p(0.43, 0.7), p(0.76, 0.32)], s);
    } else {
        ui.painter().rect_stroke(b, CornerRadius::same(3), Stroke::new(1.2, style::BORDER), egui::StrokeKind::Inside);
    }
}

/// 代码块：描边圆角卡片，顶部灰色标题栏（语言名 + 复制），下方浅底代码区。
fn code_block(ui: &mut egui::Ui, lang: &str, text: &str, ctx: Ctx) -> bool {
    let mut copied = false;
    egui::Frame::new()
        .fill(CODE_BLOCK_BG)
        .stroke(Stroke::new(1.0, CODE_BLOCK_BORDER))
        .corner_radius(CornerRadius::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::Frame::new()
                .fill(CODE_HEADER_BG)
                .corner_radius(CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 })
                .inner_margin(egui::Margin { left: 14, right: 6, top: 3, bottom: 3 })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let name = if lang.is_empty() { "code" } else { lang };
                        ui.label(egui::RichText::new(name).small().strong().color(style::MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icons::button(ui, Icon::Copy, "复制代码").clicked() {
                                ui.ctx().copy_text(text.to_string());
                                copied = true;
                            }
                        });
                    });
                });
            let (line, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
            ui.painter().rect_filled(line, CornerRadius::ZERO, CODE_BLOCK_BORDER);
            egui::Frame::new()
                .inner_margin(egui::Margin { left: 16, right: 12, top: 12, bottom: 14 })
                .show(ui, |ui| code_body(ui, lang, text, ctx));
        });
    copied
}

fn code_body(ui: &mut egui::Ui, lang: &str, text: &str, ctx: Ctx) {
    let size = ctx.size - 1.0;
    let theme = egui_extras::syntax_highlighting::CodeTheme::light(size);
    let mut job = egui_extras::syntax_highlighting::highlight(ui.ctx(), ui.style(), &theme, text, &syntax_key(lang));
    for s in &mut job.sections {
        s.format.line_height = Some((size * 1.55).round());
    }
    job.wrap.max_width = f32::INFINITY;
    egui::ScrollArea::horizontal().auto_shrink([false, true]).show(ui, |ui| {
        ui.add(egui::Label::new(job).extend().selectable(true));
    });
}

/// 围栏语言名 → syntect 可识别的扩展名（syntect 按 `Python` 名或 `py` 扩展名查找，小写全称查不到）。
fn syntax_key(lang: &str) -> String {
    let lower = lang.to_ascii_lowercase();
    let key = match lower.as_str() {
        "python" | "python3" => "py",
        "javascript" | "jsx" => "js",
        "typescript" | "tsx" => "ts",
        "rust" => "rs",
        "shell" | "bash" | "zsh" | "console" => "sh",
        "golang" => "go",
        "csharp" | "c#" => "cs",
        "c++" | "cpp" => "cpp",
        "kotlin" => "kt",
        "ruby" => "rb",
        "markdown" => "md",
        "yml" => "yaml",
        "objective-c" | "objc" => "m",
        other => other,
    };
    key.to_string()
}

/// 表格：列宽按内容自适应（见 [`column_widths`]）；白底圆角外框，表头浅灰底，行间细线。
fn table(ui: &mut egui::Ui, head: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>], ctx: Ctx) {
    let cols = head.len().max(rows.iter().map(Vec::len).max().unwrap_or(0));
    if cols == 0 {
        return;
    }
    let head_ctx = Ctx { bold: true, ..ctx };
    let pad = egui::vec2(ctx.em(CELL_PAD_X), ctx.em(CELL_PAD_Y));
    // 各列单元格不折行时的最大宽度
    let mut natural = vec![0.0_f32; cols];
    let all_rows = std::iter::once((head, head_ctx)).chain(rows.iter().map(|r| (r.as_slice(), ctx)));
    for (row, c) in all_rows {
        for (w, cell) in natural.iter_mut().zip(row) {
            let width = ui.painter().layout_job(inline_job(ui, cell, c).job).size().x;
            *w = w.max(width.ceil());
        }
    }
    let paddings = 2.0 * pad.x * cols as f32;
    // 外框描边各占 1
    let widths = column_widths(&natural, ui.available_width() - paddings - 2.0, ctx.em(MIN_COLUMN));
    let total = widths.iter().sum::<f32>() + paddings;
    egui::ScrollArea::horizontal().auto_shrink([true, true]).show(ui, |ui| {
        egui::Frame::new()
            .fill(Color32::WHITE)
            .stroke(Stroke::new(1.0, CODE_BLOCK_BORDER))
            .corner_radius(CornerRadius::same(TABLE_RADIUS))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                ui.set_width(total);
                let backdrop = ui.painter().add(egui::Shape::Noop);
                let head_rect = table_row(ui, head, &widths, pad, head_ctx);
                let r = TABLE_RADIUS - 1;
                let radius = CornerRadius { nw: r, ne: r, sw: 0, se: 0 };
                ui.painter().set(backdrop, egui::Shape::rect_filled(head_rect, radius, CODE_HEADER_BG));
                for row in rows {
                    let (line, _) = ui.allocate_exact_size(egui::vec2(total, 1.0), egui::Sense::hover());
                    ui.painter().rect_filled(line, CornerRadius::ZERO, CODE_BLOCK_BORDER);
                    table_row(ui, row, &widths, pad, ctx);
                }
            });
    });
}

/// 一行单元格：每格宽度固定，格内文字按列宽换行；返回整行矩形。
fn table_row(ui: &mut egui::Ui, cells: &[Vec<Inline>], widths: &[f32], pad: egui::Vec2, ctx: Ctx) -> egui::Rect {
    let margin = egui::Margin::symmetric(pad.x as i8, pad.y as i8);
    ui.horizontal_top(|ui| {
        for (i, w) in widths.iter().enumerate() {
            let cell_size = egui::vec2(w + 2.0 * pad.x, 0.0);
            ui.allocate_ui_with_layout(cell_size, egui::Layout::top_down(egui::Align::Min), |ui| {
                egui::Frame::new().inner_margin(margin).show(ui, |ui| {
                    ui.set_width(*w);
                    if let Some(cell) = cells.get(i) {
                        paragraph(ui, cell, ctx);
                    }
                });
            });
        }
    })
    .response
    .rect
}

/// 表格列宽：自然宽度之和不超过 `avail` 时原样返回；否则求上限 `cap` 使 Σmin(宽, cap) = `avail`，
/// 超过上限的列收窄到 `cap`。`cap` 不低于 `min`，此时表格会超出 `avail`，由外层横向滚动。
fn column_widths(natural: &[f32], avail: f32, min: f32) -> Vec<f32> {
    if natural.iter().sum::<f32>() <= avail {
        return natural.to_vec();
    }
    let mut sorted = natural.to_vec();
    sorted.sort_by(f32::total_cmp);
    let mut rest = avail;
    let mut cap = 0.0;
    for (i, w) in sorted.iter().enumerate() {
        let n = (sorted.len() - i) as f32;
        if w * n >= rest {
            cap = rest / n;
            break;
        }
        rest -= w;
    }
    let cap = cap.max(min);
    natural.iter().map(|w| w.min(cap)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Inline {
        Inline { text: text.into(), ..Default::default() }
    }

    #[test]
    fn headings_paragraphs_and_inline_styles() {
        let blocks = parse("# 标题\n\n普通**加粗**和`代码`，[链接](https://a.b)。");
        assert_eq!(blocks[0], Block::Heading(1, vec![plain("标题")]));
        let Block::Para(p) = &blocks[1] else { panic!("{blocks:?}") };
        assert_eq!(p[0], plain("普通"));
        assert!(p[1].strong && p[1].text == "加粗");
        assert!(p[3].code && p[3].text == "代码");
        assert_eq!(p[5].link.as_deref(), Some("https://a.b"));
        assert_eq!(p[5].text, "链接");
    }

    #[test]
    fn tight_and_loose_lists_and_tasks() {
        let tight = parse("1. **一**：说明\n2. 二\n");
        let Block::List { start: Some(1), items } = &tight[0] else { panic!("{tight:?}") };
        assert_eq!(items.len(), 2);
        let Block::Para(p) = &items[0].blocks[0] else { panic!("{items:?}") };
        assert!(p[0].strong && p[0].text == "一");
        assert_eq!(p[1].text, "：说明");

        let loose = parse("- 甲\n\n- 乙\n  - 子项\n");
        let Block::List { start: None, items } = &loose[0] else { panic!("{loose:?}") };
        assert_eq!(items[0].blocks, vec![Block::Para(vec![plain("甲")])]);
        assert!(matches!(&items[1].blocks[1], Block::List { start: None, items } if items.len() == 1));

        let tasks = parse("- [x] 完成\n- [ ] 待办\n");
        let Block::List { items, .. } = &tasks[0] else { panic!() };
        assert_eq!((items[0].task, items[1].task), (Some(true), Some(false)));
    }

    #[test]
    fn code_quote_table_rule() {
        let blocks = parse("```html title\n<p>a</p>\n```\n\n> 引用\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n---\n");
        assert_eq!(blocks[0], Block::Code { lang: "html".into(), text: "<p>a</p>".into() });
        assert_eq!(blocks[1], Block::Quote(vec![Block::Para(vec![plain("引用")])]));
        let Block::Table { head, rows } = &blocks[2] else { panic!("{blocks:?}") };
        assert_eq!(head, &vec![vec![plain("A")], vec![plain("B")]]);
        assert_eq!(rows, &vec![vec![vec![plain("1")], vec![plain("2")]]]);
        assert_eq!(blocks[3], Block::Rule);
    }

    #[test]
    fn fence_names_map_to_syntect_keys() {
        assert_eq!(syntax_key("Python"), "py");
        assert_eq!(syntax_key("bash"), "sh");
        assert_eq!(syntax_key("html"), "html");
    }

    #[test]
    fn soft_break_kept_as_newline() {
        assert_eq!(parse("第一行\n第二行"), vec![Block::Para(vec![plain("第一行\n第二行")])]);
    }

    // assistant-chat AC-016：块间距（正文 13pt）——段落 0.75em，标题上方 1.25em（一、二级）/ 1.1em，
    // 标题下方 0.5em，列表项内 0.3em
    #[test]
    fn block_gaps_follow_spec() {
        let para = Block::Para(vec![plain("p")]);
        let h2 = Block::Heading(2, vec![plain("h")]);
        let h3 = Block::Heading(3, vec![plain("h")]);
        let list = Block::List { start: None, items: Vec::new() };
        assert_eq!(gap_before(&para, &para, 13.0, false), 10.0);
        assert_eq!(gap_before(&para, &h2, 13.0, false), 16.0);
        assert_eq!(gap_before(&para, &h3, 13.0, false), 14.0);
        assert_eq!(gap_before(&h2, &para, 13.0, false), 7.0);
        assert_eq!(gap_before(&para, &list, 13.0, true), 4.0);
    }

    // assistant-chat AC-016：总宽不超过可用宽度时保持自然列宽；超出时从最宽的列开始收窄，且不窄于下限
    #[test]
    fn column_widths_fit_or_shrink_widest() {
        assert_eq!(column_widths(&[30.0, 50.0], 200.0, 52.0), vec![30.0, 50.0]);
        assert_eq!(column_widths(&[30.0, 300.0, 500.0], 400.0, 52.0), vec![30.0, 185.0, 185.0]);
        assert_eq!(column_widths(&[30.0, 100.0, 500.0], 400.0, 52.0), vec![30.0, 100.0, 270.0]);
        assert_eq!(column_widths(&[100.0, 100.0, 100.0], 60.0, 52.0), vec![52.0, 52.0, 52.0]);
    }
}
