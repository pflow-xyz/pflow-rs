use crate::Args;
use pflow_results::results::io::read_json;
use pflow_visualization::plotter::SvgPlotter;

pub fn run(raw: &[String]) -> Result<(), String> {
    let args = Args::parse(raw, &[]);
    let path = args.positional.first().ok_or("results file required")?;
    let results = read_json(path).map_err(|e| e.to_string())?;

    let width = args.get_f64("width", 800.0);
    let height = args.get_f64("height", 400.0);

    let mut plotter = SvgPlotter::new(width, height);
    plotter.set_title(if results.model.name.is_empty() { "pflow simulation".to_string() } else { results.model.name.clone() });
    plotter.set_x_label("time");
    plotter.set_y_label("state");

    let time = &results.data.timeseries.time.downsampled;
    let mut names: Vec<&String> = results.data.timeseries.variables.keys().collect();
    names.sort();
    for name in names {
        let series = &results.data.timeseries.variables[name];
        plotter.add_series(time.clone(), series.downsampled.clone(), name.clone(), String::new());
    }

    let svg = plotter.render();
    let output = args.get("output").ok_or("--output is required")?;
    std::fs::write(output, svg).map_err(|e| format!("write {output}: {e}"))
}
