//! Workflow SVG rendering, ported from go-pflow's
//! `visualization/workflow_svg.go`.
//!
//! Task/dependency layout is deterministic (topological level assignment,
//! then alphabetical order within a level), so this is held to structural
//! unit tests rather than a byte-identical golden the same way
//! [`crate::net`] is — go-pflow's own checked-in example SVGs, if any
//! existed, would only pin one particular `map` iteration order.

use std::collections::HashMap;
use std::fmt::Write as _;

use pflow_workflow::types::{DependencyType, JoinType, SplitType, TaskType, Workflow};

use crate::colors::escape_xml;

/// Controls workflow rendering, matching go-pflow's `WorkflowSVGOptions`.
#[derive(Clone, Debug)]
pub struct WorkflowSvgOptions {
    pub node_width: f64,
    pub node_height: f64,
    pub node_spacing_x: f64,
    pub node_spacing_y: f64,
    pub padding: f64,
    pub show_labels: bool,
    pub show_types: bool,
    pub show_join_split: bool,
    pub color_by_type: bool,
}

impl Default for WorkflowSvgOptions {
    fn default() -> Self {
        WorkflowSvgOptions {
            node_width: 120.0,
            node_height: 50.0,
            node_spacing_x: 180.0,
            node_spacing_y: 80.0,
            padding: 60.0,
            show_labels: true,
            show_types: true,
            show_join_split: true,
            color_by_type: true,
        }
    }
}

#[derive(Clone, Copy)]
struct NodePosition {
    x: f64,
    y: f64,
}

/// Renders a [`Workflow`] as an SVG document, matching go-pflow's
/// `RenderWorkflowSVG`.
pub fn render_workflow_svg(wf: &Workflow, opts: &WorkflowSvgOptions) -> String {
    let levels = assign_levels(wf);
    let positions = calculate_positions(wf, &levels, opts);

    let (mut min_x, mut min_y, mut max_x, mut max_y) = calculate_bounds(&positions, opts);
    min_x -= opts.padding;
    min_y -= opts.padding;
    max_x += opts.padding;
    max_y += opts.padding;

    let mut width = max_x - min_x;
    let mut height = max_y - min_y;
    if width < 200.0 {
        width = 200.0;
    }
    if height < 100.0 {
        height = 100.0;
    }

    let mut buf = String::new();
    let _ = writeln!(
        buf,
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{:.1} {:.1} {:.1} {:.1}" width="{:.0}" height="{:.0}">"##,
        min_x, min_y, width, height, width, height
    );
    let _ = writeln!(
        buf,
        r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="#f8f9fa" rx="8"/>"##,
        min_x, min_y, width, height
    );

    buf.push_str("<defs>");
    buf.push_str("<style>");
    buf.push_str(".task { stroke-width: 2; }");
    buf.push_str(".task-manual { fill: #e3f2fd; stroke: #1976d2; }");
    buf.push_str(".task-automatic { fill: #f3e5f5; stroke: #7b1fa2; }");
    buf.push_str(".task-decision { fill: #fff3e0; stroke: #f57c00; }");
    buf.push_str(".task-subflow { fill: #e8f5e9; stroke: #388e3c; }");
    buf.push_str(".task-default { fill: #fafafa; stroke: #666; }");
    buf.push_str(".task-start { fill: #c8e6c9; stroke: #2e7d32; }");
    buf.push_str(".task-end { fill: #ffcdd2; stroke: #c62828; }");
    buf.push_str(".dependency { stroke: #666; stroke-width: 1.5; fill: none; }");
    buf.push_str(".dependency-fs { stroke: #666; }");
    buf.push_str(".dependency-ss { stroke: #2196f3; stroke-dasharray: 5,3; }");
    buf.push_str(".dependency-ff { stroke: #4caf50; stroke-dasharray: 5,3; }");
    buf.push_str(".dependency-sf { stroke: #ff9800; stroke-dasharray: 5,3; }");
    buf.push_str(".arrowhead { fill: #666; }");
    buf.push_str(".task-label { font-family: system-ui, Arial; font-size: 12px; fill: #333; text-anchor: middle; dominant-baseline: middle; }");
    buf.push_str(".task-type { font-family: system-ui, Arial; font-size: 9px; fill: #666; text-anchor: middle; }");
    buf.push_str(".join-split { font-family: system-ui, Arial; font-size: 8px; fill: #999; text-anchor: middle; }");
    buf.push_str(".workflow-title { font-family: system-ui, Arial; font-size: 14px; font-weight: bold; fill: #333; }");
    buf.push_str("</style>");

    buf.push_str(r##"<marker id="arrowhead" markerWidth="10" markerHeight="7" refX="9" refY="3.5" orient="auto">"##);
    buf.push_str(r##"<polygon points="0 0, 10 3.5, 0 7" class="arrowhead"/>"##);
    buf.push_str("</marker>");
    buf.push_str("</defs>\n");

    if !wf.name.is_empty() {
        let _ = writeln!(
            buf,
            r##"<text x="{:.1}" y="{:.1}" class="workflow-title">{}</text>"##,
            min_x + 10.0,
            min_y + 20.0,
            escape_xml(&wf.name)
        );
    }

    for dep in &wf.dependencies {
        draw_dependency(&mut buf, dep, &positions, opts);
    }

    for (task_id, task) in &wf.tasks {
        let Some(&pos) = positions.get(task_id.as_str()) else {
            continue;
        };
        let is_start = task_id == &wf.start_task_id;
        let is_end = wf.end_task_ids.iter().any(|id| id == task_id);
        draw_task(&mut buf, task, pos, is_start, is_end, opts);
    }

    buf.push_str("</svg>\n");
    buf
}

fn assign_levels(wf: &Workflow) -> HashMap<String, i64> {
    let mut predecessors: HashMap<&str, Vec<&str>> = HashMap::new();
    for dep in &wf.dependencies {
        predecessors.entry(dep.to_task_id.as_str()).or_default().push(dep.from_task_id.as_str());
    }

    let mut levels: HashMap<String, i64> = wf.tasks.keys().map(|id| (id.clone(), 0)).collect();

    let mut changed = true;
    while changed {
        changed = false;
        for task_id in wf.tasks.keys() {
            let mut max_pred_level = -1i64;
            if let Some(preds) = predecessors.get(task_id.as_str()) {
                for &pred_id in preds {
                    if let Some(&lvl) = levels.get(pred_id) {
                        if lvl > max_pred_level {
                            max_pred_level = lvl;
                        }
                    }
                }
            }
            let new_level = max_pred_level + 1;
            if new_level > levels[task_id] {
                levels.insert(task_id.clone(), new_level);
                changed = true;
            }
        }
    }

    levels
}

fn calculate_positions(wf: &Workflow, levels: &HashMap<String, i64>, opts: &WorkflowSvgOptions) -> HashMap<String, NodePosition> {
    let mut by_level: HashMap<i64, Vec<String>> = HashMap::new();
    let mut max_level = 0i64;
    for task_id in wf.tasks.keys() {
        let level = levels.get(task_id).copied().unwrap_or(0);
        by_level.entry(level).or_default().push(task_id.clone());
        if level > max_level {
            max_level = level;
        }
    }
    for tasks in by_level.values_mut() {
        tasks.sort();
    }

    let mut positions = HashMap::new();
    for level in 0..=max_level {
        if let Some(tasks) = by_level.get(&level) {
            for (i, task_id) in tasks.iter().enumerate() {
                let x = level as f64 * opts.node_spacing_x;
                let y = i as f64 * opts.node_spacing_y;
                positions.insert(task_id.clone(), NodePosition { x, y });
            }
        }
    }
    positions
}

fn calculate_bounds(positions: &HashMap<String, NodePosition>, opts: &WorkflowSvgOptions) -> (f64, f64, f64, f64) {
    let mut first = true;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (0.0, 0.0, 0.0, 0.0);
    for pos in positions.values() {
        let node_min_x = pos.x - opts.node_width / 2.0;
        let node_max_x = pos.x + opts.node_width / 2.0;
        let node_min_y = pos.y - opts.node_height / 2.0;
        let node_max_y = pos.y + opts.node_height / 2.0;
        if first {
            min_x = node_min_x;
            max_x = node_max_x;
            min_y = node_min_y;
            max_y = node_max_y;
            first = false;
        } else {
            min_x = min_x.min(node_min_x);
            max_x = max_x.max(node_max_x);
            min_y = min_y.min(node_min_y);
            max_y = max_y.max(node_max_y);
        }
    }
    (min_x, min_y, max_x, max_y)
}

fn draw_task(
    buf: &mut String,
    task: &pflow_workflow::types::Task,
    pos: NodePosition,
    is_start: bool,
    is_end: bool,
    opts: &WorkflowSvgOptions,
) {
    let x = pos.x - opts.node_width / 2.0;
    let y = pos.y - opts.node_height / 2.0;

    let class = if is_start {
        "task task-start"
    } else if is_end {
        "task task-end"
    } else if opts.color_by_type {
        match task.typ {
            TaskType::Manual => "task task-manual",
            TaskType::Automatic => "task task-automatic",
            TaskType::Decision => "task task-decision",
            TaskType::Subflow => "task task-subflow",
        }
    } else {
        "task task-default"
    };

    if matches!(task.typ, TaskType::Decision) {
        let cx = pos.x;
        let cy = pos.y;
        let hw = opts.node_width / 2.0;
        let hh = opts.node_height / 2.0;
        let _ = writeln!(
            buf,
            r##"<polygon points="{:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1}" class="{}"/>"##,
            cx,
            cy - hh,
            cx + hw,
            cy,
            cx,
            cy + hh,
            cx - hw,
            cy,
            class
        );
    } else {
        let rx = if matches!(task.typ, TaskType::Subflow) { 0.0 } else { 5.0 };
        let _ = writeln!(
            buf,
            r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="{:.1}" class="{}"/>"##,
            x, y, opts.node_width, opts.node_height, rx, class
        );
    }

    if opts.show_labels {
        let mut label = if task.name.is_empty() { task.id.clone() } else { task.name.clone() };
        if label.chars().count() > 15 {
            label = label.chars().take(12).collect::<String>() + "...";
        }
        let _ = writeln!(buf, r##"<text x="{:.1}" y="{:.1}" class="task-label">{}</text>"##, pos.x, pos.y, escape_xml(&label));
    }

    if opts.show_types {
        let type_label = task_type_label(task.typ);
        let _ = writeln!(
            buf,
            r##"<text x="{:.1}" y="{:.1}" class="task-type">{}</text>"##,
            pos.x,
            y + opts.node_height + 10.0,
            type_label
        );
    }

    if opts.show_join_split {
        let mut indicators = String::new();
        if !matches!(task.join_type, JoinType::All) {
            indicators.push_str(join_type_label(task.join_type));
            indicators.push_str("-join");
        }
        if !matches!(task.split_type, SplitType::All) {
            if !indicators.is_empty() {
                indicators.push_str(" | ");
            }
            indicators.push_str(split_type_label(task.split_type));
            indicators.push_str("-split");
        }
        if !indicators.is_empty() {
            let _ = writeln!(buf, r##"<text x="{:.1}" y="{:.1}" class="join-split">{}</text>"##, pos.x, y - 5.0, indicators);
        }
    }
}

fn task_type_label(t: TaskType) -> &'static str {
    match t {
        TaskType::Manual => "manual",
        TaskType::Automatic => "automatic",
        TaskType::Decision => "decision",
        TaskType::Subflow => "subflow",
    }
}

fn join_type_label(t: JoinType) -> &'static str {
    match t {
        JoinType::All => "all",
        JoinType::Any => "any",
        JoinType::NOfM => "n-of-m",
    }
}

fn split_type_label(t: SplitType) -> &'static str {
    match t {
        SplitType::All => "all",
        SplitType::Exclusive => "exclusive",
        SplitType::Inclusive => "inclusive",
    }
}

fn draw_dependency(
    buf: &mut String,
    dep: &pflow_workflow::types::Dependency,
    positions: &HashMap<String, NodePosition>,
    opts: &WorkflowSvgOptions,
) {
    let (Some(&from_pos), Some(&to_pos)) = (positions.get(dep.from_task_id.as_str()), positions.get(dep.to_task_id.as_str())) else {
        return;
    };

    let (x1, y1, mut x2, y2) = match dep.typ {
        DependencyType::FinishToStart => (
            from_pos.x + opts.node_width / 2.0,
            from_pos.y,
            to_pos.x - opts.node_width / 2.0,
            to_pos.y,
        ),
        DependencyType::StartToStart => (
            from_pos.x - opts.node_width / 2.0,
            from_pos.y,
            to_pos.x - opts.node_width / 2.0,
            to_pos.y,
        ),
        DependencyType::FinishToFinish => (
            from_pos.x + opts.node_width / 2.0,
            from_pos.y,
            to_pos.x + opts.node_width / 2.0,
            to_pos.y,
        ),
        DependencyType::StartToFinish => (
            from_pos.x - opts.node_width / 2.0,
            from_pos.y,
            to_pos.x + opts.node_width / 2.0,
            to_pos.y,
        ),
    };

    let class = match dep.typ {
        DependencyType::FinishToStart => "dependency dependency-fs",
        DependencyType::StartToStart => "dependency dependency-ss",
        DependencyType::FinishToFinish => "dependency dependency-ff",
        DependencyType::StartToFinish => "dependency dependency-sf",
    };

    let arrow_offset = 10.0;
    let dx = x2 - x1;
    let dy = y2 - y1;
    let dist = (dx * dx + dy * dy).sqrt().max(1.0);
    x2 -= (dx / dist) * arrow_offset;
    let y2 = y2 - (dy / dist) * arrow_offset;

    if y1 == y2 {
        let _ = writeln!(
            buf,
            r##"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" class="{}" marker-end="url(#arrowhead)"/>"##,
            x1, y1, x2, y2, class
        );
    } else {
        let mid_x = (x1 + x2) / 2.0;
        let _ = writeln!(
            buf,
            r##"<path d="M {:.1} {:.1} C {:.1} {:.1} {:.1} {:.1} {:.1} {:.1}" class="{}" marker-end="url(#arrowhead)"/>"##,
            x1, y1, mid_x, y1, mid_x, y2, x2, y2, class
        );
    }
}

/// Renders a [`Workflow`] and writes it to `path`.
pub fn save_workflow_svg(wf: &Workflow, path: &std::path::Path, opts: &WorkflowSvgOptions) -> std::io::Result<()> {
    std::fs::write(path, render_workflow_svg(wf, opts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_workflow::types::{Dependency, Task};
    use std::time::Duration;

    fn linear_workflow() -> Workflow {
        let mut wf = Workflow {
            id: "wf".into(),
            name: "Order".into(),
            start_task_id: "receive".into(),
            end_task_ids: vec!["ship".into()],
            ..Default::default()
        };
        wf.tasks.insert(
            "receive".into(),
            Task { id: "receive".into(), name: "Receive".into(), typ: TaskType::Manual, ..Default::default() },
        );
        wf.tasks.insert(
            "validate".into(),
            Task { id: "validate".into(), name: "Validate".into(), typ: TaskType::Automatic, ..Default::default() },
        );
        wf.tasks.insert(
            "ship".into(),
            Task { id: "ship".into(), name: "Ship".into(), typ: TaskType::Manual, ..Default::default() },
        );
        wf.dependencies.push(Dependency {
            from_task_id: "receive".into(),
            to_task_id: "validate".into(),
            typ: DependencyType::FinishToStart,
            lag: Duration::ZERO,
            condition: None,
        });
        wf.dependencies.push(Dependency {
            from_task_id: "validate".into(),
            to_task_id: "ship".into(),
            typ: DependencyType::FinishToStart,
            lag: Duration::ZERO,
            condition: None,
        });
        wf
    }

    #[test]
    fn renders_well_formed_svg() {
        let svg = render_workflow_svg(&linear_workflow(), &WorkflowSvgOptions::default());
        assert!(svg.starts_with("<svg"));
        assert!(svg.trim_end().ends_with("</svg>"));
        assert!(svg.contains("viewBox="));
    }

    #[test]
    fn every_task_and_the_title_appear() {
        let svg = render_workflow_svg(&linear_workflow(), &WorkflowSvgOptions::default());
        assert!(svg.contains(">Order<"));
        assert!(svg.contains(">Receive<"));
        assert!(svg.contains(">Validate<"));
        assert!(svg.contains(">Ship<"));
    }

    #[test]
    fn start_and_end_tasks_get_their_own_class() {
        let svg = render_workflow_svg(&linear_workflow(), &WorkflowSvgOptions::default());
        assert!(svg.contains("task task-start"));
        assert!(svg.contains("task task-end"));
    }

    #[test]
    fn decision_task_renders_as_a_diamond() {
        let mut wf = linear_workflow();
        wf.tasks.get_mut("validate").unwrap().typ = TaskType::Decision;
        let svg = render_workflow_svg(&wf, &WorkflowSvgOptions::default());
        assert!(svg.contains("<polygon"));
    }

    #[test]
    fn dependency_types_map_to_distinct_classes() {
        let mut wf = linear_workflow();
        wf.dependencies[0].typ = DependencyType::StartToStart;
        let svg = render_workflow_svg(&wf, &WorkflowSvgOptions::default());
        assert!(svg.contains("dependency-ss"));
        assert!(svg.contains("dependency-fs"));
    }

    #[test]
    fn empty_workflow_still_produces_minimum_sized_svg() {
        // No tasks means the bounds default to (0,0,0,0); with the default
        // 60px padding that is 120x120 before the size floors apply, so
        // only the 200px width floor actually triggers (height stays 120).
        let svg = render_workflow_svg(&Workflow::default(), &WorkflowSvgOptions::default());
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains(r##"width="200""##));
    }

    #[test]
    fn label_is_xml_escaped() {
        let mut wf = linear_workflow();
        wf.tasks.get_mut("receive").unwrap().name = "A & B".into();
        let svg = render_workflow_svg(&wf, &WorkflowSvgOptions::default());
        assert!(svg.contains("A &amp; B"));
    }
}
