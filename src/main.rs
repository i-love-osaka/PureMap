use std::{collections::BTreeMap, fs, path::PathBuf};

use eframe::egui::{
    self, Align2, Color32, FontId, Key, Modifiers, PointerButton, Pos2, Rect, Sense, Stroke, Vec2,
};
use serde::{Deserialize, Serialize};
#[cfg(target_os = "macos")]
use std::sync::{OnceLock, mpsc};

#[cfg(target_os = "macos")]
use objc2::{
    MainThreadMarker,
    runtime::{AnyClass, AnyObject, ClassBuilder, Sel},
    sel,
};
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSApplication, NSApplicationDelegateReply};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSArray, NSString};

const BRANCH_COLORS: [Color32; 6] = [
    Color32::from_rgb(255, 138, 101),
    Color32::from_rgb(77, 182, 172),
    Color32::from_rgb(121, 134, 203),
    Color32::from_rgb(240, 98, 146),
    Color32::from_rgb(174, 213, 129),
    Color32::from_rgb(255, 213, 79),
];

#[cfg(target_os = "macos")]
static OPEN_DOCUMENT_SENDER: OnceLock<mpsc::Sender<Result<PathBuf, String>>> = OnceLock::new();

#[cfg(target_os = "macos")]
static OPEN_FILES_DELEGATE_CLASS: OnceLock<&'static AnyClass> = OnceLock::new();

#[cfg(target_os = "macos")]
unsafe extern "C-unwind" fn application_open_files(
    _delegate: &AnyObject,
    _selector: Sel,
    application: &NSApplication,
    filenames: &NSArray<NSString>,
) {
    let mut succeeded = filenames.count() > 0;
    for index in 0..filenames.count() {
        let filename = filenames.objectAtIndex(index);
        succeeded &= send_open_document_request(Ok(PathBuf::from(filename.to_string())));
    }
    application.replyToOpenOrPrint(if succeeded {
        NSApplicationDelegateReply::Success
    } else {
        NSApplicationDelegateReply::Failure
    });
}

#[cfg(target_os = "macos")]
fn send_open_document_request(request: Result<PathBuf, String>) -> bool {
    if let Some(sender) = OPEN_DOCUMENT_SENDER.get() {
        if sender.send(request).is_ok() {
            return true;
        }
    }
    eprintln!("无法将 Finder 的打开文档事件传递给应用窗口");
    false
}

#[cfg(target_os = "macos")]
fn install_open_files_handler(main_thread: MainThreadMarker) {
    let application = NSApplication::sharedApplication(main_thread);
    let delegate = application
        .delegate()
        .expect("winit has installed its NSApplication delegate");
    let delegate_object: &AnyObject = AsRef::<AnyObject>::as_ref(&*delegate);
    let delegate_class = delegate_object.class();
    let open_files_class = OPEN_FILES_DELEGATE_CLASS.get_or_init(|| {
        let mut class_builder =
            ClassBuilder::new(c"PureMapWinitApplicationDelegate", delegate_class)
                .expect("PureMap's winit delegate subclass is registered once");
        unsafe {
            class_builder.add_method(
                sel!(application:openFiles:),
                application_open_files as unsafe extern "C-unwind" fn(_, _, _, _),
            );
        }
        class_builder.register()
    });
    if !std::ptr::eq(delegate_object.class(), *open_files_class) {
        // The subclass adds no ivars and retains Winit's delegate implementation.
        let previous_class = unsafe { AnyObject::set_class(delegate_object, open_files_class) };
        assert_eq!(previous_class, delegate_class);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Theme {
    #[default]
    Auto,
    Light,
    Dark,
}

fn connection_endpoints(
    from_rect: Rect,
    from_center: Pos2,
    to_rect: Rect,
    to_center: Pos2,
) -> (Pos2, Pos2) {
    let direction = if to_center.x >= from_center.x {
        1.0
    } else {
        -1.0
    };
    let from = rect_edge_towards(
        from_rect,
        from_center,
        Pos2::new(from_center.x + direction, from_center.y),
    );
    let to = rect_edge_towards(
        to_rect,
        to_center,
        Pos2::new(to_center.x - direction, to_center.y),
    );
    (from, to)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Node {
    id: String,
    parent_id: Option<String>,
    text: String,
    #[serde(default)]
    children: Vec<String>,
    #[serde(default)]
    x: f32,
    #[serde(default)]
    y: f32,
    #[serde(default)]
    collapsed: bool,
    #[serde(default)]
    custom_position: bool,
    #[serde(default)]
    color: Option<[u8; 3]>,
    #[serde(default)]
    bold: bool,
    #[serde(default)]
    italic: bool,
    #[serde(default)]
    marked_deleted: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CanvasState {
    #[serde(default)]
    offset_x: f32,
    #[serde(default)]
    offset_y: f32,
    #[serde(default = "default_zoom")]
    zoom: f32,
}

fn default_zoom() -> f32 {
    1.0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default = "default_title")]
    title: String,
    root_id: String,
    nodes: BTreeMap<String, Node>,
    #[serde(default)]
    canvas: CanvasState,
    #[serde(default)]
    theme: Theme,
    #[serde(default = "default_language")]
    lang: String,
}

fn default_version() -> u32 {
    1
}

fn default_title() -> String {
    "未命名思维导图".to_owned()
}

fn default_language() -> String {
    "zh-Hans".to_owned()
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            offset_x: 0.0,
            offset_y: 0.0,
            zoom: 1.0,
        }
    }
}

impl Document {
    fn new() -> Self {
        let id = "root".to_owned();
        let root = Node {
            id: id.clone(),
            parent_id: None,
            text: "中心主题".to_owned(),
            children: Vec::new(),
            x: 0.0,
            y: 0.0,
            collapsed: false,
            custom_position: false,
            color: None,
            bold: true,
            italic: false,
            marked_deleted: false,
        };
        Self {
            version: 1,
            title: default_title(),
            root_id: id.clone(),
            nodes: BTreeMap::from([(id, root)]),
            canvas: CanvasState::default(),
            theme: Theme::Auto,
            lang: default_language(),
        }
    }

    fn validate(&self) -> Result<(), String> {
        let root = self
            .nodes
            .get(&self.root_id)
            .ok_or_else(|| "文件缺少有效的根节点".to_owned())?;
        if root.parent_id.is_some() {
            return Err("根节点不能包含父节点".to_owned());
        }
        for (id, node) in &self.nodes {
            if id != &node.id {
                return Err(format!("节点索引与 ID 不匹配：{id}"));
            }
            if let Some(parent) = &node.parent_id {
                let parent_node = self
                    .nodes
                    .get(parent)
                    .ok_or_else(|| format!("节点 {} 引用了不存在的父节点", node.text))?;
                if !parent_node.children.contains(id) {
                    return Err(format!("节点 {} 的父子关系不完整", node.text));
                }
            }
            for child in &node.children {
                let child_node = self
                    .nodes
                    .get(child)
                    .ok_or_else(|| format!("节点 {} 引用了不存在的子节点", node.text))?;
                if child_node.parent_id.as_deref() != Some(id.as_str()) {
                    return Err(format!("节点 {} 的子节点关系不完整", node.text));
                }
            }
        }
        Ok(())
    }

    fn from_markdown(markdown: &str) -> Self {
        let mut document = Self::new();
        let root_id = document.root_id.clone();
        let mut parents = vec![root_id.clone()];
        let mut found_root_heading = false;

        for line in markdown.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let heading_depth = trimmed
                .chars()
                .take_while(|character| *character == '#')
                .count();
            let (depth, text) =
                if heading_depth > 0 && trimmed.chars().nth(heading_depth) == Some(' ') {
                    let heading_text = trimmed[heading_depth + 1..].trim();
                    if !found_root_heading {
                        if let Some(root) = document.nodes.get_mut(&root_id) {
                            root.text = heading_text.to_owned();
                        }
                        found_root_heading = true;
                        continue;
                    }
                    (heading_depth.saturating_sub(1).max(1), heading_text)
                } else {
                    let indentation = line.len() - line.trim_start().len();
                    let content = trimmed
                        .strip_prefix("- ")
                        .or_else(|| trimmed.strip_prefix("* "))
                        .or_else(|| trimmed.strip_prefix("+ "))
                        .unwrap_or(trimmed);
                    (indentation / 2 + 1, content)
                };
            if text.is_empty() {
                continue;
            }
            let depth = depth.clamp(1, parents.len());
            parents.truncate(depth);
            let parent_id = parents.last().cloned().unwrap_or_else(|| root_id.clone());
            let id = uuid::Uuid::new_v4().to_string();
            document.nodes.insert(
                id.clone(),
                Node {
                    id: id.clone(),
                    parent_id: Some(parent_id.clone()),
                    text: text.to_owned(),
                    children: Vec::new(),
                    x: 0.0,
                    y: 0.0,
                    collapsed: false,
                    custom_position: false,
                    color: None,
                    bold: false,
                    italic: false,
                    marked_deleted: false,
                },
            );
            document
                .nodes
                .get_mut(&parent_id)
                .expect("parent is present")
                .children
                .push(id.clone());
            parents.push(id);
        }
        document.layout();
        document
    }

    fn layout(&mut self) {
        let root_id = self.root_id.clone();
        if let Some(root) = self.nodes.get_mut(&root_id) {
            root.x = 0.0;
            root.y = 0.0;
        }
        self.layout_children(&root_id, 0);
    }

    fn subtree_leaf_count(&self, id: &str) -> usize {
        let Some(node) = self.nodes.get(id) else {
            return 1;
        };
        if node.children.is_empty() || node.collapsed {
            return 1;
        }
        node.children
            .iter()
            .map(|child| self.subtree_leaf_count(child))
            .sum()
    }

    fn layout_children(&mut self, parent_id: &str, depth: usize) {
        let Some(parent) = self.nodes.get(parent_id) else {
            return;
        };
        let children = parent.children.clone();
        let parent_x = parent.x;
        let parent_y = parent.y;
        let leaf_count: usize = children
            .iter()
            .map(|child| self.subtree_leaf_count(child))
            .sum();
        if leaf_count == 0 {
            return;
        }

        let mut next_leaf = 0usize;
        for child_id in children {
            let child_leaves = self.subtree_leaf_count(&child_id);
            let y = parent_y
                + (next_leaf as f32 + (child_leaves.saturating_sub(1)) as f32 / 2.0
                    - (leaf_count.saturating_sub(1)) as f32 / 2.0)
                    * 96.0;
            next_leaf += child_leaves;

            if let Some(child) = self.nodes.get_mut(&child_id)
                && !child.custom_position
            {
                child.x = parent_x + if depth == 0 { 220.0 } else { 200.0 };
                child.y = y;
            }
            self.layout_children(&child_id, depth + 1);
        }
    }

    fn translate_subtree(&mut self, id: &str, delta: Vec2) {
        let Some(children) = self.nodes.get(id).map(|node| node.children.clone()) else {
            return;
        };
        if let Some(node) = self.nodes.get_mut(id) {
            node.x += delta.x;
            node.y += delta.y;
        }
        for child in children {
            self.translate_subtree(&child, delta);
        }
    }

    fn branch_color(&self, id: &str) -> Color32 {
        let mut current = id;
        while let Some(node) = self.nodes.get(current) {
            if let Some(color) = node.color {
                return Color32::from_rgb(color[0], color[1], color[2]);
            }
            let Some(parent_id) = node.parent_id.as_deref() else {
                break;
            };
            if parent_id == self.root_id {
                let index = self
                    .nodes
                    .get(parent_id)
                    .and_then(|p| p.children.iter().position(|child| child == current))
                    .unwrap_or(0);
                return BRANCH_COLORS[index % BRANCH_COLORS.len()];
            }
            current = parent_id;
        }
        Color32::from_rgb(91, 145, 245)
    }

    fn visible_ids(&self) -> Vec<String> {
        fn visit(doc: &Document, id: &str, out: &mut Vec<String>) {
            let Some(node) = doc.nodes.get(id) else {
                return;
            };
            out.push(id.to_owned());
            if !node.collapsed {
                for child in &node.children {
                    visit(doc, child, out);
                }
            }
        }
        let mut result = Vec::new();
        visit(self, &self.root_id, &mut result);
        result
    }

    fn search_matches(&self, query: &str) -> Vec<String> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        self.visible_ids()
            .into_iter()
            .filter(|id| {
                self.nodes
                    .get(id)
                    .is_some_and(|node| node.text.to_lowercase().contains(&query))
            })
            .collect()
    }

    fn markdown(&self) -> String {
        fn write_node(doc: &Document, id: &str, depth: usize, out: &mut String) {
            let Some(node) = doc.nodes.get(id) else {
                return;
            };
            if depth == 0 {
                out.push_str(&format!("# {}\n\n", node.text));
            } else {
                out.push_str(&format!("{}- {}\n", "  ".repeat(depth - 1), node.text));
            }
            for child in &node.children {
                write_node(doc, child, depth + 1, out);
            }
        }
        let mut result = String::new();
        write_node(self, &self.root_id, 0, &mut result);
        result
    }
}

struct PureMapApp {
    document: Document,
    selected_id: String,
    editing_id: Option<String>,
    edit_text: String,
    edit_has_undo_snapshot: bool,
    file_path: Option<PathBuf>,
    undo_stack: Vec<Document>,
    redo_stack: Vec<Document>,
    search: String,
    status: String,
    dirty: bool,
    drag_node: Option<String>,
    focus_id: Option<String>,
    show_grid: bool,
    search_open: bool,
    search_focus_pending: bool,
    about_open: bool,
    #[cfg(target_os = "macos")]
    native_menu: Option<muda::Menu>,
    #[cfg(target_os = "macos")]
    open_document_receiver: Option<mpsc::Receiver<Result<PathBuf, String>>>,
}

#[derive(Clone, Copy)]
enum ToolbarIcon {
    Undo,
    Redo,
    Add,
    AddSibling,
    AddParent,
    Delete,
    MarkDeleted,
    Search,
    Center,
    Arrange,
    ZoomIn,
    ZoomOut,
    Export,
}

fn rect_edge_towards(rect: Rect, center: Pos2, target: Pos2) -> Pos2 {
    let direction = target - center;
    if direction.length_sq() <= f32::EPSILON {
        return center;
    }

    let x_scale = if direction.x.abs() > f32::EPSILON {
        rect.width() * 0.5 / direction.x.abs()
    } else {
        f32::INFINITY
    };
    let y_scale = if direction.y.abs() > f32::EPSILON {
        rect.height() * 0.5 / direction.y.abs()
    } else {
        f32::INFINITY
    };
    center + direction * x_scale.min(y_scale)
}

fn paint_visually_centered_text(
    painter: &egui::Painter,
    center: Pos2,
    text: &str,
    font_id: FontId,
    color: Color32,
) -> Rect {
    let galley = painter.layout_no_wrap(text.to_owned(), font_id, color);
    let mesh_bounds = galley.mesh_bounds;
    let visual_offset = if mesh_bounds.is_positive() {
        mesh_bounds.center() - galley.rect.center()
    } else {
        Vec2::ZERO
    };
    let galley_pos = center - galley.rect.center().to_vec2() - visual_offset;
    let visual_rect = mesh_bounds.translate(galley_pos.to_vec2());
    painter.galley(galley_pos, galley, color);
    visual_rect
}

fn toolbar_icon_button(ui: &mut egui::Ui, label: &str, icon: ToolbarIcon) -> bool {
    toolbar_icon_button_with_state(ui, label, icon, false)
}

fn toolbar_icon_button_with_state(
    ui: &mut egui::Ui,
    label: &str,
    icon: ToolbarIcon,
    active: bool,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(58.0, 62.0), Sense::click());
    if active {
        ui.painter().rect_filled(
            rect,
            8.0,
            ui.visuals().selection.bg_fill.gamma_multiply(0.35),
        );
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, 8.0, ui.visuals().widgets.hovered.bg_fill);
    }

    let painter = ui.painter();
    let color = if response.hovered() {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().text_color()
    };
    let center = Pos2::new(rect.center().x, rect.top() + 21.0);
    let stroke = Stroke::new(1.8_f32, color);
    let line = |a, b| painter.line_segment([a, b], stroke);

    match icon {
        ToolbarIcon::Undo | ToolbarIcon::Redo => {
            let direction = if matches!(icon, ToolbarIcon::Undo) {
                -1.0
            } else {
                1.0
            };
            let points = (0..=16).map(|step| {
                let angle =
                    std::f32::consts::PI * 0.15 + std::f32::consts::PI * 1.45 * step as f32 / 16.0;
                Pos2::new(
                    center.x + direction * angle.cos() * 8.0,
                    center.y + angle.sin() * 8.0,
                )
            });
            painter.add(egui::Shape::line(points.collect(), stroke));
            let tip_x = center.x + direction * 8.0;
            line(
                Pos2::new(tip_x, center.y - 7.0),
                Pos2::new(tip_x - direction * 5.0, center.y - 2.0),
            );
            line(
                Pos2::new(tip_x, center.y - 7.0),
                Pos2::new(tip_x + direction * 5.0, center.y - 2.0),
            );
        }
        ToolbarIcon::Add => {
            painter.circle_stroke(center, 7.0, stroke);
            line(
                Pos2::new(center.x + 7.0, center.y),
                Pos2::new(center.x + 14.0, center.y),
            );
            line(
                Pos2::new(center.x + 14.0, center.y - 5.0),
                Pos2::new(center.x + 14.0, center.y + 5.0),
            );
            line(
                Pos2::new(center.x + 9.0, center.y),
                Pos2::new(center.x + 19.0, center.y),
            );
            line(
                Pos2::new(center.x + 14.0, center.y - 5.0),
                Pos2::new(center.x + 14.0, center.y + 5.0),
            );
        }
        ToolbarIcon::AddSibling => {
            line(
                Pos2::new(center.x - 7.0, center.y - 9.0),
                Pos2::new(center.x - 7.0, center.y + 9.0),
            );
            line(
                Pos2::new(center.x - 7.0, center.y),
                Pos2::new(center.x + 4.0, center.y),
            );
            painter.circle_filled(Pos2::new(center.x - 7.0, center.y - 9.0), 2.5, color);
            painter.circle_filled(Pos2::new(center.x - 7.0, center.y + 9.0), 2.5, color);
            line(
                Pos2::new(center.x + 1.0, center.y - 5.0),
                Pos2::new(center.x + 1.0, center.y + 5.0),
            );
            line(
                Pos2::new(center.x - 4.0, center.y),
                Pos2::new(center.x + 6.0, center.y),
            );
        }
        ToolbarIcon::AddParent => {
            line(
                Pos2::new(center.x - 8.0, center.y),
                Pos2::new(center.x + 5.0, center.y),
            );
            line(
                Pos2::new(center.x + 5.0, center.y - 8.0),
                Pos2::new(center.x + 5.0, center.y + 8.0),
            );
            painter.circle_filled(Pos2::new(center.x + 5.0, center.y - 8.0), 2.5, color);
            painter.circle_filled(Pos2::new(center.x + 5.0, center.y + 8.0), 2.5, color);
            line(
                Pos2::new(center.x - 3.0, center.y - 5.0),
                Pos2::new(center.x - 3.0, center.y + 5.0),
            );
            line(
                Pos2::new(center.x - 8.0, center.y),
                Pos2::new(center.x + 2.0, center.y),
            );
        }
        ToolbarIcon::Delete => {
            painter.rect_stroke(
                Rect::from_center_size(center + Vec2::new(0.0, 2.0), Vec2::new(12.0, 14.0)),
                2.0,
                stroke,
                egui::StrokeKind::Inside,
            );
            line(
                Pos2::new(center.x - 8.0, center.y - 7.0),
                Pos2::new(center.x + 8.0, center.y - 7.0),
            );
            line(
                Pos2::new(center.x - 3.0, center.y - 10.0),
                Pos2::new(center.x + 3.0, center.y - 10.0),
            );
            line(
                Pos2::new(center.x - 3.0, center.y - 2.0),
                Pos2::new(center.x - 3.0, center.y + 7.0),
            );
            line(
                Pos2::new(center.x + 3.0, center.y - 2.0),
                Pos2::new(center.x + 3.0, center.y + 7.0),
            );
        }
        ToolbarIcon::MarkDeleted => {
            painter.text(
                center,
                Align2::CENTER_CENTER,
                "A",
                FontId::proportional(17.0),
                color,
            );
            line(
                Pos2::new(center.x - 9.0, center.y + 1.0),
                Pos2::new(center.x + 9.0, center.y + 1.0),
            );
        }
        ToolbarIcon::Search => {
            painter.circle_stroke(center - Vec2::new(2.0, 2.0), 7.0, stroke);
            line(
                Pos2::new(center.x + 3.0, center.y + 3.0),
                Pos2::new(center.x + 9.0, center.y + 9.0),
            );
        }
        ToolbarIcon::Center => {
            painter.circle_stroke(center, 8.0, stroke);
            painter.circle_filled(center, 2.5, color);
            line(
                Pos2::new(center.x - 12.0, center.y),
                Pos2::new(center.x - 8.0, center.y),
            );
            line(
                Pos2::new(center.x + 8.0, center.y),
                Pos2::new(center.x + 12.0, center.y),
            );
            line(
                Pos2::new(center.x, center.y - 12.0),
                Pos2::new(center.x, center.y - 8.0),
            );
            line(
                Pos2::new(center.x, center.y + 8.0),
                Pos2::new(center.x, center.y + 12.0),
            );
        }
        ToolbarIcon::Arrange => {
            for y in [-7.0, 0.0, 7.0] {
                line(
                    Pos2::new(center.x - 9.0, center.y + y),
                    Pos2::new(center.x + 9.0, center.y + y),
                );
                painter.circle_filled(Pos2::new(center.x - 9.0, center.y + y), 2.0, color);
                painter.circle_filled(Pos2::new(center.x + 9.0, center.y + y), 2.0, color);
            }
        }
        ToolbarIcon::ZoomIn | ToolbarIcon::ZoomOut => {
            painter.circle_stroke(center - Vec2::new(2.0, 2.0), 7.0, stroke);
            line(
                Pos2::new(center.x + 3.0, center.y + 3.0),
                Pos2::new(center.x + 9.0, center.y + 9.0),
            );
            line(
                Pos2::new(center.x - 5.0, center.y - 2.0),
                Pos2::new(center.x + 1.0, center.y - 2.0),
            );
            if matches!(icon, ToolbarIcon::ZoomIn) {
                line(
                    Pos2::new(center.x - 2.0, center.y - 5.0),
                    Pos2::new(center.x - 2.0, center.y + 1.0),
                );
            }
        }
        ToolbarIcon::Export => {
            line(
                Pos2::new(center.x, center.y - 10.0),
                Pos2::new(center.x, center.y + 3.0),
            );
            line(
                Pos2::new(center.x - 5.0, center.y - 2.0),
                Pos2::new(center.x, center.y + 3.0),
            );
            line(
                Pos2::new(center.x + 5.0, center.y - 2.0),
                Pos2::new(center.x, center.y + 3.0),
            );
            painter.rect_stroke(
                Rect::from_min_max(
                    Pos2::new(center.x - 9.0, center.y + 4.0),
                    Pos2::new(center.x + 9.0, center.y + 10.0),
                ),
                2.0,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
    }

    painter.text(
        Pos2::new(rect.center().x, rect.bottom() - 9.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(11.0),
        ui.visuals().text_color(),
    );
    response.clicked()
}

fn search_dialog_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(72.0, 32.0), sense);

    let visuals = if !enabled {
        &ui.visuals().widgets.noninteractive
    } else if response.is_pointer_button_down_on() {
        &ui.visuals().widgets.active
    } else if response.hovered() {
        &ui.visuals().widgets.hovered
    } else {
        &ui.visuals().widgets.inactive
    };
    ui.painter()
        .rect_filled(rect, visuals.corner_radius, visuals.bg_fill);
    ui.painter().rect_stroke(
        rect,
        visuals.corner_radius,
        visuals.bg_stroke,
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(14.0),
        visuals.fg_stroke.color,
    );
    response.clicked() && enabled
}

impl Default for PureMapApp {
    fn default() -> Self {
        let document = Document::new();
        Self {
            selected_id: document.root_id.clone(),
            document,
            editing_id: None,
            edit_text: String::new(),
            edit_has_undo_snapshot: false,
            file_path: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            search: String::new(),
            status: "就绪 · Tab 添加子节点，Enter 添加同级节点".to_owned(),
            dirty: false,
            drag_node: None,
            focus_id: None,
            show_grid: false,
            search_open: false,
            search_focus_pending: false,
            about_open: false,
            #[cfg(target_os = "macos")]
            native_menu: None,
            #[cfg(target_os = "macos")]
            open_document_receiver: None,
        }
    }
}

impl PureMapApp {
    fn new_document(&mut self) {
        #[cfg(target_os = "macos")]
        let native_menu = self.native_menu.take();

        *self = Self::default();

        #[cfg(target_os = "macos")]
        {
            self.native_menu = native_menu;
        }
    }

    fn record_undo(&mut self) {
        self.undo_stack.push(self.document.clone());
        if self.undo_stack.len() > 200 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
        self.dirty = true;
    }

    fn undo(&mut self) {
        if let Some(previous) = self.undo_stack.pop() {
            self.redo_stack.push(self.document.clone());
            self.document = previous;
            self.selected_id = self.document.root_id.clone();
            self.dirty = true;
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(self.document.clone());
            self.document = next;
            self.selected_id = self.document.root_id.clone();
            self.dirty = true;
        }
    }

    fn center_on_node(&mut self, id: &str) {
        if let Some(node) = self.document.nodes.get(id) {
            self.document.canvas.offset_x = -node.x * self.document.canvas.zoom;
            self.document.canvas.offset_y = -node.y * self.document.canvas.zoom;
        }
    }

    fn add_node(&mut self, as_child: bool) {
        let selected = self.selected_id.clone();
        let parent_id = if as_child {
            selected.clone()
        } else {
            self.document
                .nodes
                .get(&selected)
                .and_then(|node| node.parent_id.clone())
                .unwrap_or_else(|| selected.clone())
        };
        if !self.document.nodes.contains_key(&parent_id) {
            return;
        }
        let sibling_index = if as_child {
            None
        } else {
            self.document.nodes.get(&parent_id).and_then(|parent| {
                parent
                    .children
                    .iter()
                    .position(|child| child == &selected)
                    .map(|index| index + 1)
            })
        };
        self.record_undo();
        let id = uuid::Uuid::new_v4().to_string();
        let node = Node {
            id: id.clone(),
            parent_id: Some(parent_id.clone()),
            text: String::new(),
            children: Vec::new(),
            x: 0.0,
            y: 0.0,
            collapsed: false,
            custom_position: false,
            color: None,
            bold: false,
            italic: false,
            marked_deleted: false,
        };
        self.document.nodes.insert(id.clone(), node);
        let parent = self
            .document
            .nodes
            .get_mut(&parent_id)
            .expect("parent was checked");
        if let Some(index) = sibling_index {
            parent.children.insert(index, id.clone());
        } else {
            parent.children.push(id.clone());
        }
        self.document.layout();
        self.selected_id = id.clone();
        self.center_on_node(&id);
        self.begin_edit(&id);
        self.edit_has_undo_snapshot = true;
    }

    fn add_parent(&mut self) {
        let selected = self.selected_id.clone();
        if selected == self.document.root_id {
            self.status = "根节点已经是最高层级".to_owned();
            return;
        }
        let Some(old_parent) = self
            .document
            .nodes
            .get(&selected)
            .and_then(|node| node.parent_id.clone())
        else {
            return;
        };
        self.record_undo();
        let id = uuid::Uuid::new_v4().to_string();
        let new_parent = Node {
            id: id.clone(),
            parent_id: Some(old_parent.clone()),
            text: String::new(),
            children: vec![selected.clone()],
            x: 0.0,
            y: 0.0,
            collapsed: false,
            custom_position: false,
            color: None,
            bold: false,
            italic: false,
            marked_deleted: false,
        };
        if let Some(parent) = self.document.nodes.get_mut(&old_parent)
            && let Some(index) = parent.children.iter().position(|child| child == &selected)
        {
            parent.children[index] = id.clone();
        }
        if let Some(node) = self.document.nodes.get_mut(&selected) {
            node.parent_id = Some(id.clone());
        }
        self.document.nodes.insert(id.clone(), new_parent);
        self.document.layout();
        self.selected_id = id.clone();
        self.begin_edit(&id);
        self.edit_has_undo_snapshot = true;
    }

    fn duplicate_selected(&mut self) {
        let source = self.selected_id.clone();
        let Some(parent_id) = self
            .document
            .nodes
            .get(&source)
            .and_then(|node| node.parent_id.clone())
        else {
            return;
        };
        self.record_undo();
        fn duplicate_subtree(document: &mut Document, source: &str, parent: &str) -> String {
            let original = document
                .nodes
                .get(source)
                .expect("source node exists")
                .clone();
            let id = uuid::Uuid::new_v4().to_string();
            let children = original.children.clone();
            document.nodes.insert(
                id.clone(),
                Node {
                    id: id.clone(),
                    parent_id: Some(parent.to_owned()),
                    text: original.text,
                    children: Vec::new(),
                    x: original.x,
                    y: original.y,
                    collapsed: original.collapsed,
                    custom_position: false,
                    color: original.color,
                    bold: original.bold,
                    italic: original.italic,
                    marked_deleted: original.marked_deleted,
                },
            );
            for child in children {
                let child_id = duplicate_subtree(document, &child, &id);
                document
                    .nodes
                    .get_mut(&id)
                    .expect("duplicate node exists")
                    .children
                    .push(child_id);
            }
            id
        }
        let duplicate_id = duplicate_subtree(&mut self.document, &source, &parent_id);
        if let Some(parent) = self.document.nodes.get_mut(&parent_id) {
            let index = parent
                .children
                .iter()
                .position(|child| child == &source)
                .map_or(parent.children.len(), |index| index + 1);
            parent.children.insert(index, duplicate_id.clone());
        }
        self.document.layout();
        self.selected_id = duplicate_id;
        self.dirty = true;
    }

    fn set_all_collapsed(&mut self, collapsed: bool) {
        self.record_undo();
        for node in self.document.nodes.values_mut() {
            node.collapsed = collapsed && !node.children.is_empty();
        }
    }

    fn clear_custom_positions(&mut self) {
        self.arrange_nodes();
    }

    fn arrange_nodes(&mut self) {
        self.record_undo();
        for node in self.document.nodes.values_mut() {
            node.custom_position = false;
        }
        self.document.layout();
        let visible_ids = self.document.visible_ids();
        let Some((min_x, max_x, min_y, max_y)) =
            visible_ids
                .iter()
                .fold(None, |bounds: Option<(f32, f32, f32, f32)>, id| {
                    let node = self.document.nodes.get(id)?;
                    Some(match bounds {
                        Some((min_x, max_x, min_y, max_y)) => (
                            min_x.min(node.x),
                            max_x.max(node.x),
                            min_y.min(node.y),
                            max_y.max(node.y),
                        ),
                        None => (node.x, node.x, node.y, node.y),
                    })
                })
        else {
            return;
        };
        self.document.canvas.offset_x = -(min_x + max_x) * 0.5 * self.document.canvas.zoom;
        self.document.canvas.offset_y = -(min_y + max_y) * 0.5 * self.document.canvas.zoom;
    }

    fn change_selected_color(&mut self) {
        self.record_undo();
        let current = self.document.branch_color(&self.selected_id);
        let next = BRANCH_COLORS
            .iter()
            .position(|color| *color == current)
            .map_or(0, |index| (index + 1) % BRANCH_COLORS.len());
        if let Some(node) = self.document.nodes.get_mut(&self.selected_id) {
            let color = BRANCH_COLORS[next];
            node.color = Some([color.r(), color.g(), color.b()]);
        }
    }

    fn toggle_selected_deleted(&mut self) {
        let Some(node) = self.document.nodes.get(&self.selected_id) else {
            return;
        };
        let marked_deleted = !node.marked_deleted;
        self.record_undo();
        if let Some(node) = self.document.nodes.get_mut(&self.selected_id) {
            node.marked_deleted = marked_deleted;
        }
        self.status = if marked_deleted {
            "已标记为删除".to_owned()
        } else {
            "已取消删除标记".to_owned()
        };
    }

    fn open_search(&mut self) {
        self.search_open = true;
        self.search_focus_pending = true;
    }

    fn select_search_match(&mut self, backwards: bool) {
        let matches = self.document.search_matches(&self.search);
        if matches.is_empty() {
            return;
        }

        let current = matches.iter().position(|id| id == &self.selected_id);
        let next = match (current, backwards) {
            (Some(0), true) | (None, true) => matches.len() - 1,
            (Some(index), true) => index - 1,
            (Some(index), false) => (index + 1) % matches.len(),
            (None, false) => 0,
        };
        self.selected_id = matches[next].clone();
        let selected = self.selected_id.clone();
        self.center_on_node(&selected);
    }

    #[cfg(target_os = "macos")]
    fn handle_native_menu_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = muda::MenuEvent::receiver().try_recv() {
            match event.id.as_ref() {
                "file.new" => self.new_document(),
                "file.open" => self.open_file(),
                "file.save" => self.save_file(false),
                "file.save_as" => self.save_file(true),
                "file.export_markdown" => self.export_markdown(),
                "file.quit" => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                "app.about" | "help.about" => self.about_open = true,
                "edit.undo" => self.undo(),
                "edit.redo" => self.redo(),
                "edit.duplicate" => self.duplicate_selected(),
                "edit.delete" => self.delete_selected(),
                "edit.search" => self.open_search(),
                "view.center" => {
                    self.document.canvas.offset_x = 0.0;
                    self.document.canvas.offset_y = 0.0;
                }
                "view.focus" => self.focus_id = Some(self.selected_id.clone()),
                "view.grid" => self.show_grid = !self.show_grid,
                "view.theme_auto" => self.document.theme = Theme::Auto,
                "view.theme_light" => self.document.theme = Theme::Light,
                "view.theme_dark" => self.document.theme = Theme::Dark,
                "node.child" => self.add_node(true),
                "node.sibling" => self.add_node(false),
                "node.parent" => self.add_parent(),
                "node.edit" => {
                    let id = self.selected_id.clone();
                    self.begin_edit(&id);
                }
                "node.collapse" => self.toggle_collapse(),
                "node.expand_all" => self.set_all_collapsed(false),
                "node.collapse_all" => self.set_all_collapsed(true),
                "node.clear_positions" => self.clear_custom_positions(),
                "format.color" => self.change_selected_color(),
                _ => {}
            }
        }
    }

    fn begin_edit(&mut self, id: &str) {
        self.edit_text = self
            .document
            .nodes
            .get(id)
            .map(|node| node.text.clone())
            .unwrap_or_default();
        self.editing_id = Some(id.to_owned());
        self.edit_has_undo_snapshot = false;
    }

    fn commit_edit(&mut self) {
        if let Some(id) = self.editing_id.take() {
            let updated = self.edit_text.trim().to_owned();
            let current = self.document.nodes.get(&id).map(|node| node.text.clone());
            if current.as_deref() != Some(updated.as_str()) {
                if !self.edit_has_undo_snapshot {
                    self.record_undo();
                }
                if let Some(node) = self.document.nodes.get_mut(&id) {
                    node.text = if updated.is_empty() {
                        "新节点".to_owned()
                    } else {
                        updated
                    };
                }
                self.dirty = true;
            }
            self.edit_has_undo_snapshot = false;
        }
    }

    fn delete_selected(&mut self) {
        let id = self.selected_id.clone();
        if id == self.document.root_id {
            self.status = "根节点不能删除".to_owned();
            return;
        }
        self.record_undo();
        let parent = self
            .document
            .nodes
            .get(&id)
            .and_then(|node| node.parent_id.clone());
        let mut removed = Vec::new();
        fn collect(doc: &Document, id: &str, removed: &mut Vec<String>) {
            if let Some(node) = doc.nodes.get(id) {
                for child in &node.children {
                    collect(doc, child, removed);
                }
                removed.push(id.to_owned());
            }
        }
        collect(&self.document, &id, &mut removed);
        if let Some(parent_id) = parent {
            if let Some(parent_node) = self.document.nodes.get_mut(&parent_id) {
                parent_node.children.retain(|child| child != &id);
            }
            self.selected_id = parent_id;
        }
        for removed_id in removed {
            self.document.nodes.remove(&removed_id);
        }
        self.document.layout();
    }

    fn toggle_collapse(&mut self) {
        self.record_undo();
        if let Some(node) = self.document.nodes.get_mut(&self.selected_id)
            && !node.children.is_empty()
        {
            node.collapsed = !node.collapsed;
        }
    }

    fn open_file(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PureMap / JSON / Markdown", &["puremap", "json", "md"])
            .pick_file()
        else {
            return;
        };
        self.open_document_path(path);
    }

    fn open_document_path(&mut self, path: PathBuf) {
        match fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|text| {
                if path.extension().is_some_and(|extension| extension == "md") {
                    Ok(Document::from_markdown(&text))
                } else {
                    serde_json::from_str::<Document>(&text).map_err(|error| error.to_string())
                }
            })
            .and_then(|document| {
                document.validate()?;
                Ok(document)
            }) {
            Ok(document) => {
                self.selected_id = document.root_id.clone();
                self.document = document;
                self.document.layout();
                self.file_path = Some(path);
                self.undo_stack.clear();
                self.redo_stack.clear();
                self.dirty = false;
                self.status = "文件已打开".to_owned();
            }
            Err(error) => self.status = format!("打开失败：{error}"),
        }
    }

    fn save_file(&mut self, save_as: bool) {
        let path = if save_as || self.file_path.is_none() {
            rfd::FileDialog::new()
                .add_filter("PureMap", &["puremap"])
                .set_file_name("思维导图.puremap")
                .save_file()
        } else {
            self.file_path.clone()
        };
        let Some(path) = path else { return };
        match serde_json::to_string_pretty(&self.document)
            .map_err(|error| error.to_string())
            .and_then(|json| fs::write(&path, json).map_err(|error| error.to_string()))
        {
            Ok(()) => {
                self.file_path = Some(path);
                self.dirty = false;
                self.status = "已保存".to_owned();
            }
            Err(error) => self.status = format!("保存失败：{error}"),
        }
    }

    fn export_markdown(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Markdown", &["md"])
            .set_file_name("思维导图.md")
            .save_file()
        else {
            return;
        };
        match fs::write(path, self.document.markdown()) {
            Ok(()) => self.status = "Markdown 已导出".to_owned(),
            Err(error) => self.status = format!("导出失败：{error}"),
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if self.editing_id.is_some() || ctx.wants_keyboard_input() {
            return;
        }
        let command = ctx.input(|i| i.modifiers.command);
        let pressed = |ctx: &egui::Context, key| {
            ctx.input_mut(|input| input.consume_key(Modifiers::NONE, key))
        };
        let command_pressed = |ctx: &egui::Context, key| {
            ctx.input_mut(|input| {
                input.consume_key(
                    Modifiers {
                        command: true,
                        ..Modifiers::NONE
                    },
                    key,
                )
            })
        };

        if command
            && ctx.input_mut(|input| {
                input.consume_key(
                    Modifiers {
                        command: true,
                        shift: true,
                        ..Modifiers::NONE
                    },
                    Key::A,
                )
            })
        {
            self.focus_id = Some(self.selected_id.clone());
        } else if !command
            && ctx.input_mut(|input| {
                input.consume_key(
                    Modifiers {
                        shift: true,
                        ..Modifiers::NONE
                    },
                    Key::Tab,
                )
            })
        {
            self.add_parent();
        } else if pressed(ctx, Key::Tab) {
            self.add_node(true);
        } else if pressed(ctx, Key::Enter) {
            self.add_node(false);
        } else if pressed(ctx, Key::Delete) || pressed(ctx, Key::Backspace) {
            self.delete_selected();
        } else if command && command_pressed(ctx, Key::Z) {
            self.undo();
        } else if command
            && ctx.input_mut(|input| {
                input.consume_key(
                    Modifiers {
                        command: true,
                        shift: true,
                        ..Modifiers::NONE
                    },
                    Key::Z,
                )
            })
        {
            self.redo();
        } else if command_pressed(ctx, Key::S) {
            self.save_file(false);
        } else if command_pressed(ctx, Key::O) {
            self.open_file();
        } else if command_pressed(ctx, Key::F) {
            self.open_search();
        } else if command_pressed(ctx, Key::Period) {
            self.document.canvas.offset_x = 0.0;
            self.document.canvas.offset_y = 0.0;
        } else if command_pressed(ctx, Key::Slash) {
            self.toggle_collapse();
        } else if command_pressed(ctx, Key::D) {
            self.duplicate_selected();
        } else if pressed(ctx, Key::F2) {
            let id = self.selected_id.clone();
            self.begin_edit(&id);
        } else if command_pressed(ctx, Key::K) {
            self.focus_id = if self.focus_id.is_some() {
                None
            } else {
                Some(self.selected_id.clone())
            };
        } else if command_pressed(ctx, Key::Equals) {
            self.document.canvas.zoom = (self.document.canvas.zoom * 1.12).clamp(0.1, 4.0);
        } else if command_pressed(ctx, Key::Minus) {
            self.document.canvas.zoom = (self.document.canvas.zoom / 1.12).clamp(0.1, 4.0);
        } else if command_pressed(ctx, Key::Num0) {
            self.document.canvas.zoom = 1.0;
        }
    }

    fn theme_colors(&self, ctx: &egui::Context) -> (Color32, Color32, Color32) {
        let dark = match self.document.theme {
            Theme::Auto => ctx.style().visuals.dark_mode,
            Theme::Light => false,
            Theme::Dark => true,
        };
        if dark {
            (
                Color32::from_rgb(26, 26, 29),
                Color32::from_rgb(34, 34, 38),
                Color32::from_rgb(240, 240, 243),
            )
        } else {
            (
                Color32::from_rgb(247, 247, 248),
                Color32::WHITE,
                Color32::from_rgb(37, 39, 43),
            )
        }
    }
}

impl eframe::App for PureMapApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        self.handle_native_menu_events(ctx);
        #[cfg(target_os = "macos")]
        while let Some(path) = self
            .open_document_receiver
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            match path {
                Ok(path) => self.open_document_path(path),
                Err(error) => self.status = format!("打开失败：{error}"),
            }
        }
        self.handle_shortcuts(ctx);
        let (canvas_bg, node_bg, text_color) = self.theme_colors(ctx);
        if self.document.theme == Theme::Dark
            || (self.document.theme == Theme::Auto && ctx.style().visuals.dark_mode)
        {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        #[cfg(not(target_os = "macos"))]
        {
            egui::TopBottomPanel::top("menu_bar")
                .exact_height(30.0)
                .frame(
                    egui::Frame::new()
                        .fill(node_bg)
                        .inner_margin(egui::Margin::symmetric(10, 2)),
                )
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.menu_button("文件", |ui| {
                            if ui.button("新建　⌘N").clicked() {
                                self.new_document();
                                ui.close_menu();
                            }
                            if ui.button("打开…　⌘O").clicked() {
                                self.open_file();
                                ui.close_menu();
                            }
                            if ui.button("保存　⌘S").clicked() {
                                self.save_file(false);
                                ui.close_menu();
                            }
                            if ui.button("另存为…　⇧⌘S").clicked() {
                                self.save_file(true);
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("导出 Markdown…").clicked() {
                                self.export_markdown();
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("退出").clicked() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                ui.close_menu();
                            }
                        });
                        ui.menu_button("编辑", |ui| {
                            if ui.button("撤销　⌘Z").clicked() {
                                self.undo();
                                ui.close_menu();
                            }
                            if ui.button("重做　⇧⌘Z").clicked() {
                                self.redo();
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("复制分支　⌘D").clicked() {
                                self.duplicate_selected();
                                ui.close_menu();
                            }
                            if ui.button("删除　⌫").clicked() {
                                self.delete_selected();
                                ui.close_menu();
                            }
                            if ui.button("搜索　⌘F").clicked() {
                                self.open_search();
                                ui.close_menu();
                            }
                        });
                        ui.menu_button("视图", |ui| {
                            if ui.checkbox(&mut self.show_grid, "显示网格").changed() {
                                ui.close_menu();
                            }
                            if ui.button("居中　⌘.").clicked() {
                                self.document.canvas.offset_x = 0.0;
                                self.document.canvas.offset_y = 0.0;
                                ui.close_menu();
                            }
                            if ui.button("焦点模式　⌘K").clicked() {
                                self.focus_id = Some(self.selected_id.clone());
                                ui.close_menu();
                            }
                            ui.separator();
                            ui.label("主题");
                            ui.selectable_value(&mut self.document.theme, Theme::Auto, "自动");
                            ui.selectable_value(&mut self.document.theme, Theme::Light, "浅色");
                            ui.selectable_value(&mut self.document.theme, Theme::Dark, "深色");
                        });
                        ui.menu_button("节点", |ui| {
                            if ui.button("添加子节点　Tab").clicked() {
                                self.add_node(true);
                                ui.close_menu();
                            }
                            if ui.button("添加同级节点　↵").clicked() {
                                self.add_node(false);
                                ui.close_menu();
                            }
                            if ui.button("添加父节点　⇧Tab").clicked() {
                                self.add_parent();
                                ui.close_menu();
                            }
                            if ui.button("编辑　F2").clicked() {
                                let id = self.selected_id.clone();
                                self.begin_edit(&id);
                                ui.close_menu();
                            }
                            if ui.button("折叠/展开　⌘/").clicked() {
                                self.toggle_collapse();
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("展开全部").clicked() {
                                self.set_all_collapsed(false);
                                ui.close_menu();
                            }
                            if ui.button("折叠全部").clicked() {
                                self.set_all_collapsed(true);
                                ui.close_menu();
                            }
                            if ui.button("自动整理").clicked() {
                                self.arrange_nodes();
                                ui.close_menu();
                            }
                        });
                        ui.menu_button("格式", |ui| {
                            if ui.button("更改分支颜色").clicked() {
                                self.change_selected_color();
                                ui.close_menu();
                            }
                        });
                        ui.menu_button("帮助", |ui| {
                            if ui.button("关于 PureMap").clicked() {
                                self.about_open = true;
                                ui.close_menu();
                            }
                            ui.label("PureMap · 离线思维导图");
                            ui.label("Tab 添加子节点 · Enter 添加同级");
                            ui.label("⌘Z 撤销 · ⌘S 保存");
                        });
                    });
                });
        }

        egui::TopBottomPanel::top("toolbar")
            .exact_height(76.0)
            .frame(
                egui::Frame::new()
                    .fill(node_bg)
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::horizontal()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.horizontal_centered(|ui| {
                            if toolbar_icon_button(ui, "导出", ToolbarIcon::Export) {
                                self.export_markdown();
                            }
                            ui.separator();
                            if toolbar_icon_button(ui, "撤销", ToolbarIcon::Undo) {
                                self.undo();
                            }
                            if toolbar_icon_button(ui, "重做", ToolbarIcon::Redo) {
                                self.redo();
                            }
                            ui.separator();
                            if toolbar_icon_button(ui, "子节点", ToolbarIcon::Add) {
                                self.add_node(true);
                            }
                            if toolbar_icon_button(ui, "同级", ToolbarIcon::AddSibling) {
                                self.add_node(false);
                            }
                            if toolbar_icon_button(ui, "父节点", ToolbarIcon::AddParent) {
                                self.add_parent();
                            }
                            if toolbar_icon_button(ui, "删除", ToolbarIcon::Delete) {
                                self.delete_selected();
                            }
                            if toolbar_icon_button(ui, "整理", ToolbarIcon::Arrange) {
                                self.arrange_nodes();
                            }
                            let selected_marked_deleted = self
                                .document
                                .nodes
                                .get(&self.selected_id)
                                .is_some_and(|node| node.marked_deleted);
                            let mark_deleted_label = if selected_marked_deleted {
                                "取消删除"
                            } else {
                                "标记删除"
                            };
                            if toolbar_icon_button_with_state(
                                ui,
                                mark_deleted_label,
                                ToolbarIcon::MarkDeleted,
                                selected_marked_deleted,
                            ) {
                                self.toggle_selected_deleted();
                            }
                            ui.separator();
                            if toolbar_icon_button(ui, "搜索", ToolbarIcon::Search) {
                                self.open_search();
                            }
                            if toolbar_icon_button(ui, "居中", ToolbarIcon::Center) {
                                self.document.canvas.offset_x = 0.0;
                                self.document.canvas.offset_y = 0.0;
                            }
                            if toolbar_icon_button(ui, "缩小", ToolbarIcon::ZoomOut) {
                                self.document.canvas.zoom =
                                    (self.document.canvas.zoom / 1.12).clamp(0.1, 4.0);
                            }
                            ui.label(format!("{:.0}%", self.document.canvas.zoom * 100.0));
                            if toolbar_icon_button(ui, "放大", ToolbarIcon::ZoomIn) {
                                self.document.canvas.zoom =
                                    (self.document.canvas.zoom * 1.12).clamp(0.1, 4.0);
                            }
                        });
                    });
            });

        egui::TopBottomPanel::bottom("status")
            .exact_height(26.0)
            .frame(
                egui::Frame::new()
                    .fill(node_bg)
                    .inner_margin(egui::Margin::symmetric(12, 4)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(if self.dirty { "● 未保存" } else { "✓" });
                    ui.separator();
                    ui.label(&self.status);
                    if let Some(path) = &self.file_path {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(path.file_name().unwrap_or_default().to_string_lossy());
                        });
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(canvas_bg))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let painter = ui.painter_at(rect);
                if self.show_grid {
                    let spacing = 24.0 * self.document.canvas.zoom;
                    if spacing > 6.0 {
                        let mut x = rect.left();
                        while x < rect.right() {
                            painter.line_segment(
                                [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                                Stroke::new(0.5_f32, Color32::from_gray(220)),
                            );
                            x += spacing;
                        }
                        let mut y = rect.top();
                        while y < rect.bottom() {
                            painter.line_segment(
                                [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
                                Stroke::new(0.5_f32, Color32::from_gray(220)),
                            );
                            y += spacing;
                        }
                    }
                }
                let canvas_response =
                    ui.interact(rect, egui::Id::new("canvas"), Sense::click_and_drag());
                if canvas_response.dragged_by(PointerButton::Secondary)
                    || (canvas_response.dragged() && ui.input(|input| input.key_down(Key::Space)))
                {
                    let delta = canvas_response.drag_delta();
                    self.document.canvas.offset_x += delta.x;
                    self.document.canvas.offset_y += delta.y;
                }
                let zoom_delta = ui.input(|input| input.zoom_delta());
                if (zoom_delta - 1.0).abs() > 0.001 {
                    self.document.canvas.zoom =
                        (self.document.canvas.zoom * zoom_delta).clamp(0.1, 4.0);
                }
                let scroll = ui.input(|input| input.raw_scroll_delta);
                if ui.input(|input| input.modifiers.command) && scroll.y != 0.0 {
                    self.document.canvas.zoom =
                        (self.document.canvas.zoom * (scroll.y * 0.001).exp()).clamp(0.1, 4.0);
                } else if canvas_response.hovered() && scroll != Vec2::ZERO {
                    self.document.canvas.offset_x += scroll.x;
                    self.document.canvas.offset_y += scroll.y;
                }

                let center = rect.center()
                    + Vec2::new(self.document.canvas.offset_x, self.document.canvas.offset_y);
                let visible = self.document.visible_ids();
                let draw_ids: Vec<String> = if let Some(focus) = &self.focus_id {
                    let mut subtree = Vec::new();
                    fn append(doc: &Document, id: &str, ids: &mut Vec<String>) {
                        if let Some(node) = doc.nodes.get(id) {
                            ids.push(id.to_owned());
                            if !node.collapsed {
                                for child in &node.children {
                                    append(doc, child, ids);
                                }
                            }
                        }
                    }
                    append(&self.document, focus, &mut subtree);
                    subtree
                } else {
                    visible
                };
                let positions: BTreeMap<String, (Pos2, Rect)> = draw_ids
                    .iter()
                    .filter_map(|id| {
                        self.document.nodes.get(id).map(|node| {
                            let point =
                                center + Vec2::new(node.x, node.y) * self.document.canvas.zoom;
                            let depth = node_depth(&self.document, id);
                            let width = (if depth == 0 { 180.0 } else { 154.0 })
                                * self.document.canvas.zoom.clamp(0.75, 1.4);
                            let height = (if depth == 0 { 44.0 } else { 36.0 })
                                * self.document.canvas.zoom.clamp(0.75, 1.4);
                            let node_rect = Rect::from_center_size(point, Vec2::new(width, height));
                            (id.clone(), (point, node_rect))
                        })
                    })
                    .collect();

                for id in &draw_ids {
                    let Some(node) = self.document.nodes.get(id) else {
                        continue;
                    };
                    if let Some(parent_id) = &node.parent_id
                        && let (Some((from_center, from_rect)), Some((to_center, to_rect))) =
                            (positions.get(parent_id), positions.get(id))
                    {
                        let (from, to) =
                            connection_endpoints(*from_rect, *from_center, *to_rect, *to_center);
                        let color = self.document.branch_color(id);
                        let control_distance = (to.x - from.x).abs() * 0.48;
                        let direction = if to.x >= from.x { 1.0 } else { -1.0 };
                        let control1 = Pos2::new(from.x + direction * control_distance, from.y);
                        let control2 = Pos2::new(to.x - direction * control_distance, to.y);
                        painter.add(egui::Shape::CubicBezier(
                            egui::epaint::CubicBezierShape::from_points_stroke(
                                [from, control1, control2, to],
                                false,
                                Color32::TRANSPARENT,
                                Stroke::new(
                                    2.2 * self.document.canvas.zoom,
                                    color.gamma_multiply(0.75),
                                ),
                            ),
                        ));
                    }
                }

                for id in &draw_ids {
                    let Some((point, node_rect)) = positions.get(id).copied() else {
                        continue;
                    };
                    let Some(node) = self.document.nodes.get(id).cloned() else {
                        continue;
                    };
                    if !node_rect.intersects(rect) {
                        continue;
                    }
                    let color = self.document.branch_color(id);
                    let selected = *id == self.selected_id;
                    let matching = !self.search.trim().is_empty()
                        && node
                            .text
                            .to_lowercase()
                            .contains(&self.search.to_lowercase());
                    let background = if node.marked_deleted {
                        Color32::from_gray(if ui.visuals().dark_mode { 48 } else { 228 })
                    } else if node.parent_id.is_none() {
                        color.gamma_multiply(0.18)
                    } else {
                        node_bg
                    };
                    painter.rect(
                        node_rect,
                        12.0,
                        background,
                        Stroke::new(
                            if selected {
                                2.2_f32
                            } else if matching {
                                2.0_f32
                            } else {
                                1.0_f32
                            },
                            if selected || matching {
                                Color32::from_rgb(75, 130, 245)
                            } else if node.marked_deleted {
                                Color32::from_gray(128)
                            } else {
                                color.gamma_multiply(0.55)
                            },
                        ),
                        egui::StrokeKind::Inside,
                    );
                    let font_size = if node.parent_id.is_none() { 18.0 } else { 15.0 }
                        * self.document.canvas.zoom.clamp(0.75, 1.4);
                    let node_text_color = if node.marked_deleted {
                        Color32::from_gray(if ui.visuals().dark_mode { 155 } else { 120 })
                    } else {
                        text_color
                    };
                    let text_rect = paint_visually_centered_text(
                        &painter,
                        point,
                        if node.text.is_empty() {
                            "新节点"
                        } else {
                            &node.text
                        },
                        FontId::proportional(font_size),
                        node_text_color,
                    );
                    if node.marked_deleted {
                        painter.line_segment(
                            [
                                Pos2::new(text_rect.left(), point.y),
                                Pos2::new(text_rect.right(), point.y),
                            ],
                            Stroke::new(
                                1.5 * self.document.canvas.zoom.clamp(0.75, 1.4),
                                node_text_color,
                            ),
                        );
                    }
                    if !node.children.is_empty() {
                        let toggle_pos = Pos2::new(point.x, node_rect.bottom() + 8.0);
                        painter.circle_filled(toggle_pos, 7.0, color);
                        painter.text(
                            toggle_pos,
                            Align2::CENTER_CENTER,
                            if node.collapsed { "+" } else { "−" },
                            FontId::proportional(11.0),
                            Color32::WHITE,
                        );
                    }

                    let response = ui.interact(
                        node_rect,
                        egui::Id::new(("node", id)),
                        Sense::click_and_drag(),
                    );
                    if response.clicked() {
                        self.selected_id = id.clone();
                    }
                    if response.double_clicked() {
                        self.selected_id = id.clone();
                        self.begin_edit(id);
                    }
                    if response.dragged() {
                        let change = response.drag_delta();
                        if change != Vec2::ZERO {
                            self.document
                                .translate_subtree(id, change / self.document.canvas.zoom);
                            if let Some(node) = self.document.nodes.get_mut(id) {
                                node.custom_position = true;
                            }
                            self.drag_node = Some(id.clone());
                            self.dirty = true;
                        }
                    }
                    if response.drag_stopped() && self.drag_node.as_deref() == Some(id.as_str()) {
                        self.drag_node = None;
                    }
                    if !node.children.is_empty() {
                        let toggle_rect = Rect::from_center_size(
                            Pos2::new(point.x, node_rect.bottom() + 8.0),
                            Vec2::splat(18.0),
                        );
                        if ui
                            .interact(toggle_rect, egui::Id::new(("collapse", id)), Sense::click())
                            .clicked()
                        {
                            self.selected_id = id.clone();
                            self.record_undo();
                            if let Some(node) = self.document.nodes.get_mut(id) {
                                node.collapsed = !node.collapsed;
                            }
                        }
                    }

                    let add_scale = self.document.canvas.zoom.clamp(0.75, 1.4);
                    let add_center = Pos2::new(node_rect.right() + 10.0 * add_scale, point.y);
                    let add_radius = 8.0 * add_scale;
                    painter.circle_filled(add_center, add_radius, color);
                    painter.line_segment(
                        [
                            add_center - Vec2::new(add_radius * 0.45, 0.0),
                            add_center + Vec2::new(add_radius * 0.45, 0.0),
                        ],
                        Stroke::new(1.5 * add_scale, Color32::WHITE),
                    );
                    painter.line_segment(
                        [
                            add_center - Vec2::new(0.0, add_radius * 0.45),
                            add_center + Vec2::new(0.0, add_radius * 0.45),
                        ],
                        Stroke::new(1.5 * add_scale, Color32::WHITE),
                    );
                    let add_rect =
                        Rect::from_center_size(add_center, Vec2::splat(22.0 * add_scale));
                    if ui
                        .interact(add_rect, egui::Id::new(("add-child", id)), Sense::click())
                        .on_hover_text("添加子节点")
                        .clicked()
                    {
                        self.selected_id = id.clone();
                        self.add_node(true);
                    }
                }

                if let Some(id) = self.editing_id.clone()
                    && let Some((_, edit_rect)) = positions.get(&id)
                {
                    let mut edit_rect = *edit_rect;
                    edit_rect = edit_rect.shrink2(Vec2::new(8.0, 7.0));
                    let edit_galley = ui.painter().layout_no_wrap(
                        self.edit_text.clone(),
                        FontId::proportional(15.0),
                        text_color,
                    );
                    if edit_galley.mesh_bounds.is_positive() {
                        let visual_offset =
                            edit_galley.mesh_bounds.center().y - edit_galley.rect.center().y;
                        edit_rect = edit_rect.translate(Vec2::new(0.0, -visual_offset));
                    }
                    let response = ui.put(
                        edit_rect,
                        egui::TextEdit::multiline(&mut self.edit_text)
                            .horizontal_align(egui::Align::Center)
                            .vertical_align(egui::Align::Center)
                            .desired_rows(1)
                            .font(FontId::proportional(15.0))
                            .margin(Vec2::ZERO),
                    );
                    response.request_focus();
                    let confirm =
                        ui.input(|input| input.key_pressed(Key::Enter) && !input.modifiers.shift);
                    let cancel = ui.input(|input| input.key_pressed(Key::Escape));
                    if confirm || response.lost_focus() {
                        self.commit_edit();
                    } else if cancel {
                        self.editing_id = None;
                    }
                }

                if canvas_response.clicked() && !canvas_response.dragged() {
                    self.selected_id = self.document.root_id.clone();
                }
                if ui.input(|input| input.pointer.any_released()) {
                    self.drag_node = None;
                }
            });

        if self.search_open {
            let mut open = self.search_open;
            egui::Window::new("搜索节点")
                .id(egui::Id::new("search_window"))
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .fixed_size(Vec2::new(440.0, 380.0))
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("查找思维导图中的节点");
                    ui.add_space(6.0);
                    let mut search_next = false;
                    ui.horizontal(|ui| {
                        let input_width = (ui.available_width() - 80.0).clamp(120.0, 336.0);
                        let response = ui.add_sized(
                            [input_width, 32.0],
                            egui::TextEdit::singleline(&mut self.search)
                                .id(egui::Id::new("search_input"))
                                .hint_text("输入关键词")
                                .vertical_align(egui::Align::Center)
                                .margin(egui::Margin::symmetric(10, 4)),
                        );
                        if self.search_focus_pending {
                            response.request_focus();
                            self.search_focus_pending = false;
                        }
                        search_next = response.has_focus()
                            && ui.input(|input| {
                                input.key_pressed(Key::Enter) && !input.modifiers.shift
                            });
                        if search_dialog_button(ui, "清除", true) {
                            self.search.clear();
                            self.search_focus_pending = true;
                        }
                    });
                    let matches = self.document.search_matches(&self.search);
                    if search_next {
                        self.select_search_match(false);
                    }
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.label(format!("搜索结果 · {} 个", matches.len()));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if search_dialog_button(ui, "下一个", !matches.is_empty()) {
                                self.select_search_match(false);
                            }
                            if search_dialog_button(ui, "上一个", !matches.is_empty()) {
                                self.select_search_match(true);
                            }
                        });
                    });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .max_height(220.0)
                        .show(ui, |ui| {
                            if matches.is_empty() {
                                ui.add_space(12.0);
                                ui.centered_and_justified(|ui| {
                                    ui.label(if self.search.trim().is_empty() {
                                        "输入关键词以搜索节点"
                                    } else {
                                        "没有找到匹配的节点"
                                    });
                                });
                            } else {
                                for id in &matches {
                                    let Some(node) = self.document.nodes.get(id) else {
                                        continue;
                                    };
                                    let depth = node_depth(&self.document, id);
                                    let response = ui.selectable_label(
                                        self.selected_id == *id,
                                        format!("{}{}", "　".repeat(depth), node.text),
                                    );
                                    if response.clicked() {
                                        self.selected_id = id.clone();
                                        self.document.canvas.offset_x =
                                            -node.x * self.document.canvas.zoom;
                                        self.document.canvas.offset_y =
                                            -node.y * self.document.canvas.zoom;
                                    }
                                }
                            }
                        });
                });
            self.search_open = open;
        }

        if self.about_open {
            let mut open = self.about_open;
            let mut close_requested = false;
            egui::Window::new("关于 PureMap")
                .id(egui::Id::new("about_window"))
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .title_bar(false)
                .fixed_size(Vec2::new(440.0, 300.0))
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), 40.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.label(egui::RichText::new("关于 PureMap").size(14.0));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let (button_rect, response) =
                                        ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
                                    if response.hovered() {
                                        ui.painter().rect_filled(
                                            button_rect,
                                            4.0,
                                            ui.visuals().widgets.hovered.bg_fill,
                                        );
                                    }
                                    let center = button_rect.center();
                                    let half = 5.0;
                                    let stroke = Stroke::new(1.5_f32, ui.visuals().text_color());
                                    ui.painter().line_segment(
                                        [center + Vec2::splat(-half), center + Vec2::splat(half)],
                                        stroke,
                                    );
                                    ui.painter().line_segment(
                                        [
                                            center + Vec2::new(-half, half),
                                            center + Vec2::new(half, -half),
                                        ],
                                        stroke,
                                    );
                                    if response.clicked() {
                                        close_requested = true;
                                    }
                                },
                            );
                        },
                    );
                    ui.separator();
                    let body_size = ui.available_size();
                    ui.allocate_ui_with_layout(
                        body_size,
                        egui::Layout::top_down(egui::Align::Center)
                            .with_main_align(egui::Align::Center),
                        |ui| {
                            ui.label(egui::RichText::new("PureMap").size(22.0));
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(format!("版本 {}", env!("CARGO_PKG_VERSION")))
                                    .size(16.0),
                            );
                            ui.add_space(12.0);
                            ui.label(egui::RichText::new("By Mr.Ben").size(16.0));
                            ui.hyperlink_to(
                                egui::RichText::new("https://mrben.net").size(16.0),
                                "https://mrben.net",
                            );
                        },
                    );
                });
            if close_requested {
                open = false;
            }
            self.about_open = open;
        }

        if ctx.input(|input| input.key_pressed(Key::Escape)) && self.focus_id.is_some() {
            self.focus_id = None;
        }
        if self.focus_id.is_some() {
            egui::Area::new(egui::Id::new("focus_breadcrumb"))
                .fixed_pos(Pos2::new(18.0, 68.0))
                .show(ctx, |ui| {
                    if ui.button("← 返回完整导图").clicked() {
                        self.focus_id = None;
                    }
                });
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

#[cfg(target_os = "macos")]
fn add_native_menu_command(
    submenu: &muda::Submenu,
    id: &str,
    title: &str,
    shortcut: Option<(muda::accelerator::Code, bool)>,
) -> std::io::Result<()> {
    use muda::accelerator::{Accelerator, Modifiers};

    let accelerator = shortcut.map(|(key, shift)| {
        let modifiers = if shift {
            Modifiers::SUPER | Modifiers::SHIFT
        } else {
            Modifiers::SUPER
        };
        Accelerator::new(Some(modifiers), key)
    });
    let item = muda::MenuItem::with_id(id, title, true, accelerator);
    submenu
        .append(&item)
        .map_err(|error| std::io::Error::other(error.to_string()))
}

#[cfg(target_os = "macos")]
fn add_native_menu_separator(submenu: &muda::Submenu) -> std::io::Result<()> {
    let separator = muda::PredefinedMenuItem::separator();
    submenu
        .append(&separator)
        .map_err(|error| std::io::Error::other(error.to_string()))
}

#[cfg(target_os = "macos")]
fn create_native_menu() -> std::io::Result<muda::Menu> {
    use muda::accelerator::Code;
    use muda::{Menu, PredefinedMenuItem, Submenu};

    let menu = Menu::new();

    let app_menu = Submenu::new("PureMap", true);
    add_native_menu_command(&app_menu, "app.about", "关于 PureMap", None)?;
    add_native_menu_separator(&app_menu)?;
    let hide = PredefinedMenuItem::hide(Some("隐藏 PureMap"));
    app_menu.append(&hide).map_err(menu_error)?;
    let hide_others = PredefinedMenuItem::hide_others(Some("隐藏其他"));
    app_menu.append(&hide_others).map_err(menu_error)?;
    let show_all = PredefinedMenuItem::show_all(Some("显示全部"));
    app_menu.append(&show_all).map_err(menu_error)?;
    add_native_menu_separator(&app_menu)?;
    add_native_menu_command(
        &app_menu,
        "file.quit",
        "退出 PureMap",
        Some((Code::KeyQ, false)),
    )?;
    menu.append(&app_menu).map_err(menu_error)?;

    let file = Submenu::new("文件", true);
    add_native_menu_command(&file, "file.new", "新建", Some((Code::KeyN, false)))?;
    add_native_menu_command(&file, "file.open", "打开…", Some((Code::KeyO, false)))?;
    add_native_menu_separator(&file)?;
    add_native_menu_command(&file, "file.save", "保存", Some((Code::KeyS, false)))?;
    add_native_menu_command(&file, "file.save_as", "另存为…", Some((Code::KeyS, true)))?;
    add_native_menu_separator(&file)?;
    add_native_menu_command(&file, "file.export_markdown", "导出 Markdown…", None)?;
    menu.append(&file).map_err(menu_error)?;

    let edit = Submenu::new("编辑", true);
    add_native_menu_command(&edit, "edit.undo", "撤销", Some((Code::KeyZ, false)))?;
    add_native_menu_command(&edit, "edit.redo", "重做", Some((Code::KeyZ, true)))?;
    add_native_menu_separator(&edit)?;
    add_native_menu_command(
        &edit,
        "edit.duplicate",
        "复制分支",
        Some((Code::KeyD, false)),
    )?;
    add_native_menu_command(&edit, "edit.delete", "删除", Some((Code::Backspace, false)))?;
    add_native_menu_command(&edit, "edit.search", "查找…", Some((Code::KeyF, false)))?;
    menu.append(&edit).map_err(menu_error)?;

    let view = Submenu::new("显示", true);
    add_native_menu_command(&view, "view.center", "居中", Some((Code::Period, false)))?;
    add_native_menu_command(&view, "view.focus", "焦点模式", Some((Code::KeyK, false)))?;
    add_native_menu_command(&view, "view.grid", "显示/隐藏网格", None)?;
    add_native_menu_separator(&view)?;
    add_native_menu_command(&view, "view.theme_auto", "自动主题", None)?;
    add_native_menu_command(&view, "view.theme_light", "浅色主题", None)?;
    add_native_menu_command(&view, "view.theme_dark", "深色主题", None)?;
    menu.append(&view).map_err(menu_error)?;

    let node = Submenu::new("节点", true);
    add_native_menu_command(&node, "node.child", "添加子节点", Some((Code::Tab, false)))?;
    add_native_menu_command(
        &node,
        "node.sibling",
        "添加同级节点",
        Some((Code::Enter, false)),
    )?;
    add_native_menu_command(&node, "node.parent", "添加父节点", Some((Code::Tab, true)))?;
    add_native_menu_command(&node, "node.edit", "编辑节点", Some((Code::F2, false)))?;
    add_native_menu_command(
        &node,
        "node.collapse",
        "折叠/展开",
        Some((Code::Slash, false)),
    )?;
    add_native_menu_separator(&node)?;
    add_native_menu_command(&node, "node.expand_all", "展开全部", None)?;
    add_native_menu_command(&node, "node.collapse_all", "折叠全部", None)?;
    add_native_menu_command(&node, "node.clear_positions", "清除自定义位置", None)?;
    menu.append(&node).map_err(menu_error)?;

    let format = Submenu::new("格式", true);
    add_native_menu_command(&format, "format.color", "更改分支颜色", None)?;
    menu.append(&format).map_err(menu_error)?;

    let window = Submenu::new("窗口", true);
    let minimize = PredefinedMenuItem::minimize(Some("最小化"));
    window.append(&minimize).map_err(menu_error)?;
    let close = PredefinedMenuItem::close_window(Some("关闭窗口"));
    window.append(&close).map_err(menu_error)?;
    menu.append(&window).map_err(menu_error)?;

    let help = Submenu::new("帮助", true);
    add_native_menu_command(&help, "help.about", "关于 PureMap", None)?;
    menu.append(&help).map_err(menu_error)?;

    menu.init_for_nsapp();
    window.set_as_windows_menu_for_nsapp();
    help.set_as_help_menu_for_nsapp();
    Ok(menu)
}

#[cfg(target_os = "macos")]
fn menu_error(error: muda::Error) -> std::io::Error {
    std::io::Error::other(error.to_string())
}

fn node_depth(document: &Document, id: &str) -> usize {
    let mut depth = 0;
    let mut current = id;
    while let Some(parent) = document
        .nodes
        .get(current)
        .and_then(|node| node.parent_id.as_deref())
    {
        depth += 1;
        current = parent;
    }
    depth
}

fn configure_fonts(ctx: &egui::Context) -> Result<(), std::io::Error> {
    let mut fonts = egui::FontDefinitions::default();

    #[cfg(target_os = "macos")]
    {
        let font_path = [
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/PingFang.ttc",
        ]
        .into_iter()
        .find(|path| std::path::Path::new(path).is_file())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "macOS Chinese system font was not found",
            )
        })?;
        let font_data = egui::FontData::from_owned(fs::read(font_path)?);
        fonts
            .font_data
            .insert("macos_chinese".to_owned(), std::sync::Arc::new(font_data));
        fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .expect("egui provides a proportional font family")
            .insert(0, "macos_chinese".to_owned());
        fonts
            .families
            .get_mut(&egui::FontFamily::Monospace)
            .expect("egui provides a monospace font family")
            .push("macos_chinese".to_owned());
    }

    ctx.set_fonts(fonts);
    Ok(())
}

fn puremap_path_from_args(args: impl IntoIterator<Item = std::ffi::OsString>) -> Option<PathBuf> {
    args.into_iter().map(PathBuf::from).find(|path| {
        path.extension()
            .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("puremap"))
    })
}

#[cfg(target_os = "macos")]
fn main() -> eframe::Result<()> {
    let open_path = puremap_path_from_args(std::env::args_os().skip(1));
    #[cfg(target_os = "macos")]
    let (open_document_sender, open_document_receiver) = mpsc::channel();
    #[cfg(target_os = "macos")]
    OPEN_DOCUMENT_SENDER
        .set(open_document_sender)
        .expect("open-document handler sender is initialized once");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([760.0, 500.0])
            .with_title("PureMap")
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!(
                    "../packaging/macos/PureMap.iconset/icon_128x128@2x.png"
                ))
                .expect("embedded PureMap icon is a valid PNG"),
            ),
        ..Default::default()
    };
    eframe::run_native(
        "PureMap",
        options,
        Box::new(move |creation_context| {
            configure_fonts(&creation_context.egui_ctx)?;
            let mut app = PureMapApp::default();
            if let Some(path) = open_path {
                app.open_document_path(path);
            }
            #[cfg(target_os = "macos")]
            {
                let main_thread =
                    MainThreadMarker::new().expect("application runs on the main thread");
                install_open_files_handler(main_thread);
                app.native_menu = Some(create_native_menu()?);
                app.open_document_receiver = Some(open_document_receiver);
            }
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configures_font_definitions() {
        configure_fonts(&egui::Context::default()).unwrap();
    }

    #[test]
    fn new_document_has_a_valid_root_and_markdown_outline() {
        let document = Document::new();
        document.validate().unwrap();
        assert_eq!(document.nodes.len(), 1);
        assert_eq!(document.markdown(), "# 中心主题\n\n");
    }

    #[test]
    fn layout_positions_nested_nodes_and_markdown_preserves_hierarchy() {
        let mut document = Document::new();
        let mut root = document.nodes.get(&document.root_id).unwrap().clone();
        root.children = vec!["branch".to_owned()];
        document.nodes.insert(root.id.clone(), root);
        document.nodes.insert(
            "branch".to_owned(),
            Node {
                id: "branch".to_owned(),
                parent_id: Some(document.root_id.clone()),
                text: "分支".to_owned(),
                children: vec!["leaf".to_owned()],
                x: 0.0,
                y: 0.0,
                collapsed: false,
                custom_position: false,
                color: None,
                bold: false,
                italic: false,
                marked_deleted: false,
            },
        );
        document.nodes.insert(
            "leaf".to_owned(),
            Node {
                id: "leaf".to_owned(),
                parent_id: Some("branch".to_owned()),
                text: "叶节点".to_owned(),
                children: Vec::new(),
                x: 0.0,
                y: 0.0,
                collapsed: false,
                custom_position: false,
                color: None,
                bold: false,
                italic: false,
                marked_deleted: false,
            },
        );

        document.validate().unwrap();
        document.layout();
        assert_eq!(document.nodes["branch"].x, 220.0);
        assert_eq!(document.nodes["leaf"].x, 420.0);
        assert_eq!(document.markdown(), "# 中心主题\n\n- 分支\n  - 叶节点\n");
    }

    #[test]
    fn child_layout_prefers_the_right_side_of_each_parent() {
        let mut document = Document::new();
        let root_id = document.root_id.clone();
        let mut root = document.nodes[&root_id].clone();
        root.children = vec!["left".to_owned(), "right".to_owned()];
        document.nodes.insert(root_id, root);
        for id in ["left", "right"] {
            document.nodes.insert(
                id.to_owned(),
                Node {
                    id: id.to_owned(),
                    parent_id: Some("root".to_owned()),
                    text: id.to_owned(),
                    children: Vec::new(),
                    x: 0.0,
                    y: 0.0,
                    collapsed: false,
                    custom_position: false,
                    color: None,
                    bold: false,
                    italic: false,
                    marked_deleted: false,
                },
            );
        }

        document.layout();
        assert_eq!(document.nodes["left"].x, 220.0);
        assert_eq!(document.nodes["right"].x, 220.0);
        assert_eq!(document.nodes["left"].y, -48.0);
        assert_eq!(document.nodes["right"].y, 48.0);
    }

    #[test]
    fn translating_a_node_moves_its_subtree_together() {
        let mut document = Document::from_markdown("# Plan\n\n- Branch\n  - Leaf");
        let root_id = document.root_id.clone();
        let branch_id = document.nodes[&root_id].children[0].clone();
        let leaf_id = document.nodes[&branch_id].children[0].clone();
        let root_position = (document.nodes[&root_id].x, document.nodes[&root_id].y);
        let branch_position = (document.nodes[&branch_id].x, document.nodes[&branch_id].y);
        let leaf_position = (document.nodes[&leaf_id].x, document.nodes[&leaf_id].y);
        document.nodes.get_mut(&leaf_id).unwrap().custom_position = true;

        document.translate_subtree(&branch_id, Vec2::new(32.0, -24.0));

        assert_eq!(
            (document.nodes[&root_id].x, document.nodes[&root_id].y),
            root_position
        );
        assert_eq!(
            (document.nodes[&branch_id].x, document.nodes[&branch_id].y),
            (branch_position.0 + 32.0, branch_position.1 - 24.0)
        );
        assert_eq!(
            (document.nodes[&leaf_id].x, document.nodes[&leaf_id].y),
            (leaf_position.0 + 32.0, leaf_position.1 - 24.0)
        );
        assert!(document.nodes[&leaf_id].custom_position);
    }

    #[test]
    fn connection_endpoints_start_at_node_borders() {
        let rect = Rect::from_center_size(Pos2::ZERO, Vec2::new(100.0, 40.0));
        assert_eq!(
            rect_edge_towards(rect, Pos2::ZERO, Pos2::new(200.0, 0.0)),
            Pos2::new(50.0, 0.0)
        );
        assert_eq!(
            rect_edge_towards(rect, Pos2::ZERO, Pos2::new(0.0, -200.0)),
            Pos2::new(0.0, -20.0)
        );
    }

    #[test]
    fn child_connection_enters_the_middle_of_the_child_frame() {
        let parent_rect = Rect::from_center_size(Pos2::new(0.0, 0.0), Vec2::new(100.0, 40.0));
        let child_center = Pos2::new(200.0, 60.0);
        let child_rect = Rect::from_center_size(child_center, Vec2::new(100.0, 40.0));

        let (start, end) = connection_endpoints(parent_rect, Pos2::ZERO, child_rect, child_center);

        assert_eq!(start, Pos2::new(50.0, 0.0));
        assert_eq!(end, Pos2::new(150.0, 60.0));
    }

    #[test]
    fn search_matches_only_visible_nodes_case_insensitively() {
        let document = Document::from_markdown("# Plan\n\n- Learn Rust\n  - ownership\n- Ship");
        let matches = document.search_matches("RUST");

        assert_eq!(matches.len(), 1);
        assert_eq!(document.nodes[&matches[0]].text, "Learn Rust");
        assert!(document.search_matches("missing").is_empty());
        assert!(document.search_matches(" ").is_empty());
    }

    #[test]
    fn puremap_json_round_trip_keeps_document_data() {
        let document = Document::new();
        let serialized = serde_json::to_string(&document).unwrap();
        assert!(serialized.contains("\"rootId\""));
        let restored: Document = serde_json::from_str(&serialized).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored.root_id, document.root_id);
        assert_eq!(restored.nodes[&restored.root_id].text, "中心主题");
    }

    #[test]
    fn reads_the_product_document_json_shape() {
        let serialized = r#"{
            "version": 1,
            "title": "PureMap 示例",
            "rootId": "root",
            "nodes": {
                "root": {"id":"root","parentId":null,"text":"PureMap","children":["a"],"x":0,"y":0,"collapsed":false},
                "a": {"id":"a","parentId":"root","text":"分支","children":[],"x":220,"y":-80,"collapsed":false}
            },
            "canvas": {"offsetX":0,"offsetY":0,"zoom":1},
            "theme": "auto",
            "lang": "zh-Hans"
        }"#;
        let document: Document = serde_json::from_str(serialized).unwrap();
        document.validate().unwrap();
        assert_eq!(document.nodes["a"].parent_id.as_deref(), Some("root"));
    }

    #[test]
    fn launch_arguments_select_a_puremap_document_path() {
        let path = puremap_path_from_args([
            std::ffi::OsString::from("-psn_0_12345"),
            std::ffi::OsString::from("/maps/example.PUREMAP"),
            std::ffi::OsString::from("/maps/ignored.json"),
        ]);

        assert_eq!(path, Some(PathBuf::from("/maps/example.PUREMAP")));
        assert_eq!(
            puremap_path_from_args([std::ffi::OsString::from("-psn_0_12345")]),
            None
        );
    }

    #[test]
    fn enter_adds_a_sibling_between_the_selected_node_and_its_next_sibling() {
        let mut app = PureMapApp::default();
        let root_id = app.document.root_id.clone();
        for _ in 0..2 {
            app.selected_id = root_id.clone();
            app.add_node(true);
            app.editing_id = None;
        }

        let original_children = app.document.nodes[&root_id].children.clone();
        app.selected_id = original_children[0].clone();
        app.add_node(false);

        let children = &app.document.nodes[&root_id].children;
        assert_eq!(children.len(), 3);
        assert_eq!(children[0], original_children[0]);
        assert_eq!(children[1], app.selected_id);
        assert_eq!(children[2], original_children[1]);
        assert!(
            app.document.nodes[&children[0]].y < app.document.nodes[&children[1]].y
                && app.document.nodes[&children[1]].y < app.document.nodes[&children[2]].y
        );
        assert_eq!(app.document.visible_ids().len(), app.document.nodes.len());
        app.document.validate().unwrap();

        let selected = &app.document.nodes[&app.selected_id];
        assert_eq!(
            app.document.canvas.offset_y,
            -selected.y * app.document.canvas.zoom
        );
    }

    #[test]
    fn arranging_nodes_resets_custom_positions_and_centers_the_layout() {
        let mut app = PureMapApp {
            document: Document::from_markdown("# Plan\n\n- First\n  - Nested\n- Second"),
            ..PureMapApp::default()
        };
        app.document.canvas.zoom = 1.5;
        app.document.canvas.offset_x = 200.0;
        app.document.canvas.offset_y = -100.0;
        for node in app.document.nodes.values_mut() {
            node.custom_position = true;
            node.x += 500.0;
            node.y -= 300.0;
        }

        app.arrange_nodes();

        assert!(
            app.document
                .nodes
                .values()
                .all(|node| !node.custom_position)
        );
        let visible_ids = app.document.visible_ids();
        let min_x = visible_ids
            .iter()
            .map(|id| app.document.nodes[id].x)
            .fold(f32::INFINITY, f32::min);
        let max_x = visible_ids
            .iter()
            .map(|id| app.document.nodes[id].x)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = visible_ids
            .iter()
            .map(|id| app.document.nodes[id].y)
            .fold(f32::INFINITY, f32::min);
        let max_y = visible_ids
            .iter()
            .map(|id| app.document.nodes[id].y)
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!(
            app.document.canvas.offset_x,
            -(min_x + max_x) * 0.5 * app.document.canvas.zoom
        );
        assert_eq!(
            app.document.canvas.offset_y,
            -(min_y + max_y) * 0.5 * app.document.canvas.zoom
        );
        assert_eq!(app.document.nodes[&app.document.root_id].x, 0.0);

        app.undo();
        assert!(app.document.nodes.values().all(|node| node.custom_position));
        assert_eq!(app.document.canvas.offset_x, 200.0);
        assert_eq!(app.document.canvas.offset_y, -100.0);
    }

    #[test]
    fn markdown_import_builds_nested_nodes() {
        let document = Document::from_markdown("# 计划\n\n- 学习\n  - Rust\n- 发布\n");
        document.validate().unwrap();
        let root = &document.nodes[&document.root_id];
        assert_eq!(root.text, "计划");
        assert_eq!(root.children.len(), 2);
        let learning = &document.nodes[&root.children[0]];
        assert_eq!(learning.text, "学习");
        assert_eq!(document.nodes[&learning.children[0]].text, "Rust");
        assert_eq!(document.nodes[&root.children[1]].text, "发布");
    }

    #[test]
    fn deleted_marker_toggles_and_is_saved_with_the_node() {
        let mut app = PureMapApp::default();
        let root_id = app.document.root_id.clone();

        app.toggle_selected_deleted();
        assert!(app.document.nodes[&root_id].marked_deleted);
        let serialized = serde_json::to_string(&app.document).unwrap();
        assert!(serialized.contains("\"markedDeleted\":true"));

        app.toggle_selected_deleted();
        assert!(!app.document.nodes[&root_id].marked_deleted);
        app.undo();
        assert!(app.document.nodes[&root_id].marked_deleted);
    }

    #[test]
    fn editing_existing_node_is_undoable() {
        let mut app = PureMapApp::default();
        let root_id = app.document.root_id.clone();
        app.begin_edit(&root_id);
        app.edit_text = "已修改".to_owned();
        app.commit_edit();
        assert_eq!(app.document.nodes[&root_id].text, "已修改");
        app.undo();
        assert_eq!(app.document.nodes[&root_id].text, "中心主题");
    }
}
