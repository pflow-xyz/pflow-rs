//! SVG line-plot rendering for ODE solutions, ported from go-pflow's
//! `plotter/svg.go`.
//!
//! Uses [`crate::colors::escape_minimal`] rather than
//! [`crate::colors::escape_xml`] for title/label/legend text — matching
//! go-pflow's `petri.Escape`, which only escapes `&`/`<`/`>` (see that
//! module's doc comment for why the two escapers stay separate).

use std::fmt::Write as _;

use pflow_solver::Solution;

use crate::colors::escape_minimal;

/// One data series to plot.
#[derive(Clone, Debug, Default)]
pub struct Series {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub label: String,
    pub color: String,
}

/// Metadata about the last rendered plot — margins, scale ranges and the
/// series drawn — for callers building interactive overlays (crosshairs,
/// tooltips) on top of the static SVG.
#[derive(Clone, Debug, Default)]
pub struct PlotData {
    pub plot_id: String,
    pub margin_top: f64,
    pub margin_right: f64,
    pub margin_bottom: f64,
    pub margin_left: f64,
    pub plot_width: f64,
    pub plot_height: f64,
    pub xmin: f64,
    pub xmax: f64,
    pub ymin: f64,
    pub ymax: f64,
    pub series: Vec<Series>,
}

const DEFAULT_PALETTE: [&str; 8] = [
    "#e41a1c", "#377eb8", "#4daf4a", "#984ea3", "#ff7f00", "#ffff33", "#a65628", "#f781bf",
];

/// Builds and renders SVG line plots with customizable styling, matching
/// go-pflow's `SVGPlotter`.
pub struct SvgPlotter {
    pub width: f64,
    pub height: f64,
    margin_top: f64,
    margin_right: f64,
    margin_bottom: f64,
    margin_left: f64,
    plot_width: f64,
    plot_height: f64,
    pub title: String,
    pub x_label: String,
    pub y_label: String,
    pub series: Vec<Series>,
    pub last_plot: Option<PlotData>,
}

impl SvgPlotter {
    pub fn new(width: f64, height: f64) -> Self {
        let (margin_top, margin_right, margin_bottom, margin_left) = (40.0, 30.0, 50.0, 60.0);
        let plot_width = width - margin_left - margin_right;
        let plot_height = height - margin_top - margin_bottom;
        SvgPlotter {
            width,
            height,
            margin_top,
            margin_right,
            margin_bottom,
            margin_left,
            plot_width,
            plot_height,
            title: String::new(),
            x_label: "Time".to_string(),
            y_label: "Value".to_string(),
            series: Vec::new(),
            last_plot: None,
        }
    }

    pub fn set_title(&mut self, t: impl Into<String>) -> &mut Self {
        self.title = t.into();
        self
    }

    pub fn set_x_label(&mut self, s: impl Into<String>) -> &mut Self {
        self.x_label = s.into();
        self
    }

    pub fn set_y_label(&mut self, s: impl Into<String>) -> &mut Self {
        self.y_label = s.into();
        self
    }

    /// Adds a data series. An empty `color` picks the next color from the
    /// default 8-color palette, cycling.
    pub fn add_series(&mut self, x: Vec<f64>, y: Vec<f64>, label: impl Into<String>, color: impl Into<String>) -> &mut Self {
        let color = color.into();
        let color = if color.is_empty() {
            DEFAULT_PALETTE[self.series.len() % DEFAULT_PALETTE.len()].to_string()
        } else {
            color
        };
        self.series.push(Series { x, y, label: label.into(), color });
        self
    }

    /// Renders the SVG and stores its metadata in `last_plot`.
    pub fn render(&mut self) -> String {
        let mut xmin = f64::INFINITY;
        let mut xmax = f64::NEG_INFINITY;
        let mut ymin = f64::INFINITY;
        let mut ymax = f64::NEG_INFINITY;

        for s in &self.series {
            for i in 0..s.x.len() {
                let x = s.x[i];
                let y = s.y[i];
                if x < xmin {
                    xmin = x;
                }
                if x > xmax {
                    xmax = x;
                }
                if y < ymin {
                    ymin = y;
                }
                if y > ymax {
                    ymax = y;
                }
            }
        }

        if xmin.is_infinite() || xmax.is_infinite() {
            xmin = 0.0;
            xmax = 1.0;
        }
        if ymin.is_infinite() || ymax.is_infinite() {
            ymin = 0.0;
            ymax = 1.0;
        }

        let mut xrange = xmax - xmin;
        if xrange == 0.0 {
            xrange = 1.0;
        }
        let mut yrange = ymax - ymin;
        if yrange == 0.0 {
            yrange = 1.0;
        }

        xmin -= xrange * 0.05;
        xmax += xrange * 0.05;
        ymin -= yrange * 0.1;
        ymax += yrange * 0.1;

        let sx = |x: f64| self.margin_left + ((x - xmin) / (xmax - xmin)) * self.plot_width;
        let sy = |y: f64| self.margin_top + self.plot_height - ((y - ymin) / (ymax - ymin)) * self.plot_height;

        let plot_id = format!("plot_{}", (1_000_000.0 * (xmin + xmax + ymin + ymax).abs()).round() as i64);

        let mut sb = String::new();
        let _ = write!(
            sb,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" id="{}">"##,
            self.width as i64, self.height as i64, plot_id
        );
        let _ = write!(
            sb,
            r##"<rect width="{}" height="{}" fill="#f8f9fa" rx="8"/>"##,
            self.width as i64, self.height as i64
        );

        if !self.title.is_empty() {
            let _ = write!(
                sb,
                r##"<text x="{:.6}" y="25" text-anchor="middle" font-family="Arial, sans-serif" font-size="16" font-weight="bold">{}</text>"##,
                self.width / 2.0,
                escape_minimal(&self.title)
            );
        }

        // Axes.
        let _ = write!(
            sb,
            r##"<line x1="{:.6}" y1="{:.6}" x2="{:.6}" y2="{:.6}" stroke="#333" stroke-width="2"/>"##,
            self.margin_left,
            self.margin_top,
            self.margin_left,
            self.margin_top + self.plot_height
        );
        let _ = write!(
            sb,
            r##"<line x1="{:.6}" y1="{:.6}" x2="{:.6}" y2="{:.6}" stroke="#333" stroke-width="2"/>"##,
            self.margin_left,
            self.margin_top + self.plot_height,
            self.margin_left + self.plot_width,
            self.margin_top + self.plot_height
        );

        // Axis labels.
        let _ = write!(
            sb,
            r##"<text x="{:.6}" y="{:.6}" text-anchor="middle" font-family="Arial, sans-serif" font-size="12">{}</text>"##,
            self.margin_left + self.plot_width / 2.0,
            self.height - 10.0,
            escape_minimal(&self.x_label)
        );
        let _ = write!(
            sb,
            r##"<text x="15" y="{:.6}" text-anchor="middle" font-family="Arial, sans-serif" font-size="12" transform="rotate(-90, 15, {:.6})">{}</text>"##,
            self.margin_top + self.plot_height / 2.0,
            self.margin_top + self.plot_height / 2.0,
            escape_minimal(&self.y_label)
        );

        // Grid and ticks.
        let num_x_ticks = 5;
        let num_y_ticks = 5;
        for i in 0..=num_x_ticks {
            let x = xmin + (xmax - xmin) * (i as f64) / (num_x_ticks as f64);
            let px = sx(x);
            let _ = write!(
                sb,
                r##"<line x1="{:.6}" y1="{:.6}" x2="{:.6}" y2="{:.6}" stroke="#333" stroke-width="1"/>"##,
                px,
                self.margin_top + self.plot_height,
                px,
                self.margin_top + self.plot_height + 5.0
            );
            let _ = write!(
                sb,
                r##"<text x="{:.6}" y="{:.6}" text-anchor="middle" font-family="Arial, sans-serif" font-size="10">{:.1}</text>"##,
                px,
                self.margin_top + self.plot_height + 20.0,
                x
            );
            let _ = write!(
                sb,
                r##"<line x1="{:.6}" y1="{:.6}" x2="{:.6}" y2="{:.6}" stroke="#ddd" stroke-width="0.5"/>"##,
                px,
                self.margin_top,
                px,
                self.margin_top + self.plot_height
            );
        }
        for i in 0..=num_y_ticks {
            let y = ymin + (ymax - ymin) * (i as f64) / (num_y_ticks as f64);
            let py = sy(y);
            let _ = write!(
                sb,
                r##"<line x1="{:.6}" y1="{:.6}" x2="{:.6}" y2="{:.6}" stroke="#333" stroke-width="1"/>"##,
                self.margin_left - 5.0,
                py,
                self.margin_left,
                py
            );
            let _ = write!(
                sb,
                r##"<text x="{:.6}" y="{:.6}" text-anchor="end" font-family="Arial, sans-serif" font-size="10">{:.1}</text>"##,
                self.margin_left - 10.0,
                py + 4.0,
                y
            );
            let _ = write!(
                sb,
                r##"<line x1="{:.6}" y1="{:.6}" x2="{:.6}" y2="{:.6}" stroke="#ddd" stroke-width="0.5"/>"##,
                self.margin_left,
                py,
                self.margin_left + self.plot_width,
                py
            );
        }

        // Series.
        for s in &self.series {
            if s.x.is_empty() {
                continue;
            }
            let mut path = String::new();
            for i in 0..s.x.len() {
                let px = sx(s.x[i]);
                let py = sy(s.y[i]);
                if i == 0 {
                    let _ = write!(path, "M{:.6},{:.6}", px, py);
                } else {
                    let _ = write!(path, " L{:.6},{:.6}", px, py);
                }
            }
            let _ = write!(sb, r##"<path d="{}" stroke="{}" stroke-width="2" fill="none"/>"##, path, s.color);
        }

        // Legend.
        let has_label = self.series.iter().any(|s| !s.label.is_empty());
        if has_label {
            let mut legend_y = self.margin_top + 10.0;
            for s in &self.series {
                if s.label.is_empty() {
                    continue;
                }
                let x1 = self.width - self.margin_right - 50.0;
                let x2 = self.width - self.margin_right - 30.0;
                let _ = write!(
                    sb,
                    r##"<line x1="{:.6}" y1="{:.6}" x2="{:.6}" y2="{:.6}" stroke="{}" stroke-width="2"/>"##,
                    x1, legend_y, x2, legend_y, s.color
                );
                let _ = write!(
                    sb,
                    r##"<text x="{:.6}" y="{:.6}" font-family="Arial, sans-serif" font-size="10">{}</text>"##,
                    x2 + 5.0,
                    legend_y + 4.0,
                    escape_minimal(&s.label)
                );
                legend_y += 20.0;
            }
        }

        // Crosshair group for interactivity (hidden by default).
        let _ = write!(sb, r##"<g id="{}_crosshair" style="display:none;">"##, plot_id);
        let _ = write!(
            sb,
            r##"<line id="{}_line" x1="0" y1="{:.6}" x2="0" y2="{:.6}" stroke="#666" stroke-width="1" stroke-dasharray="4,4"/>"##,
            plot_id,
            self.margin_top,
            self.margin_top + self.plot_height
        );
        sb.push_str(r##"<rect id="tooltip_bg" x="0" y="0" rx="4" ry="4" fill="white" stroke="#666" stroke-width="1" opacity="0.95"/>"##);
        sb.push_str(r##"<text id="tooltip_text" x="0" y="0" font-family="Arial, sans-serif" font-size="11" fill="#333"></text>"##);
        sb.push_str("</g>");

        // Overlay rectangle for potential interactivity.
        let _ = write!(
            sb,
            r##"<rect id="{}_overlay" x="{:.6}" y="{:.6}" width="{:.6}" height="{:.6}" fill="transparent" style="cursor:crosshair;"/>"##,
            plot_id, self.margin_left, self.margin_top, self.plot_width, self.plot_height
        );

        sb.push_str("</svg>");

        self.last_plot = Some(PlotData {
            plot_id,
            margin_top: self.margin_top,
            margin_right: self.margin_right,
            margin_bottom: self.margin_bottom,
            margin_left: self.margin_left,
            plot_width: self.plot_width,
            plot_height: self.plot_height,
            xmin,
            xmax,
            ymin,
            ymax,
            series: self.series.clone(),
        });

        sb
    }
}

/// Convenience wrapper plotting an ODE [`Solution`]. `variables = None`
/// plots every state variable, matching go-pflow's `PlotSolution`.
pub fn plot_solution(
    sol: &Solution,
    variables: Option<&[String]>,
    width: f64,
    height: f64,
    title: &str,
    xlabel: &str,
    ylabel: &str,
) -> (String, Option<PlotData>) {
    let mut plotter = SvgPlotter::new(width, height);
    if !title.is_empty() {
        plotter.set_title(title);
    }
    if !xlabel.is_empty() {
        plotter.set_x_label(xlabel);
    }
    if !ylabel.is_empty() {
        plotter.set_y_label(ylabel);
    }

    let owned;
    let vars_to_plot: &[String] = match variables {
        Some(v) => v,
        None => {
            owned = sol.state_labels.clone();
            &owned
        }
    };

    for vn in vars_to_plot {
        let y = sol.get_variable(vn);
        plotter.add_series(sol.t.clone(), y, vn.clone(), String::new());
    }

    let svg = plotter.render();
    (svg, plotter.last_plot.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_well_formed_svg() {
        let mut p = SvgPlotter::new(600.0, 400.0);
        p.add_series(vec![0.0, 1.0, 2.0], vec![0.0, 1.0, 0.5], "S", "");
        let svg = p.render();
        assert!(svg.starts_with("<svg"));
        assert!(svg.trim_end().ends_with("</svg>"));
        assert!(svg.contains("<path"));
    }

    #[test]
    fn stores_plot_metadata_after_render() {
        let mut p = SvgPlotter::new(600.0, 400.0);
        p.add_series(vec![0.0, 1.0], vec![0.0, 1.0], "S", "");
        p.render();
        let data = p.last_plot.as_ref().expect("last_plot set");
        assert_eq!(data.series.len(), 1);
        assert!(data.xmax > data.xmin);
    }

    #[test]
    fn empty_series_falls_back_to_unit_range() {
        let mut p = SvgPlotter::new(600.0, 400.0);
        let svg = p.render();
        assert!(svg.starts_with("<svg"));
        let data = p.last_plot.as_ref().unwrap();
        // xrange/yrange defaulted to [0,1] before padding.
        assert!(data.xmax > data.xmin);
        assert!(data.ymax > data.ymin);
    }

    #[test]
    fn default_palette_cycles_when_color_omitted() {
        let mut p = SvgPlotter::new(600.0, 400.0);
        for i in 0..10 {
            p.add_series(vec![0.0], vec![0.0], format!("s{i}"), "");
        }
        assert_eq!(p.series[0].color, p.series[8].color);
        assert_ne!(p.series[0].color, p.series[1].color);
    }

    #[test]
    fn explicit_color_is_kept() {
        let mut p = SvgPlotter::new(600.0, 400.0);
        p.add_series(vec![0.0], vec![0.0], "s", "#123456");
        assert_eq!(p.series[0].color, "#123456");
    }

    #[test]
    fn title_and_labels_are_minimally_escaped() {
        let mut p = SvgPlotter::new(600.0, 400.0);
        p.set_title("A & <B>");
        p.add_series(vec![0.0, 1.0], vec![0.0, 1.0], "L & R", "");
        let svg = p.render();
        assert!(svg.contains("A &amp; &lt;B&gt;"));
        assert!(svg.contains("L &amp; R"));
    }

    #[test]
    fn legend_only_drawn_when_a_series_has_a_label() {
        let mut unlabeled = SvgPlotter::new(600.0, 400.0);
        unlabeled.add_series(vec![0.0, 1.0], vec![0.0, 1.0], "", "");
        let svg_unlabeled = unlabeled.render();

        let mut labeled = SvgPlotter::new(600.0, 400.0);
        labeled.add_series(vec![0.0, 1.0], vec![0.0, 1.0], "Series A", "");
        let svg_labeled = labeled.render();

        assert!(!svg_unlabeled.contains("Series A"));
        assert!(svg_labeled.contains("Series A"));
    }
}
