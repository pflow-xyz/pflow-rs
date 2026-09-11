use crate::model_io::load_petri_net;
use crate::Args;
use pflow_visualization::net::render_petri_net_svg;

pub fn run(raw: &[String]) -> Result<(), String> {
    let args = Args::parse(raw, &[]);
    let path = args.positional.first().ok_or("model file required")?;
    let net = load_petri_net(path)?;
    let svg = render_petri_net_svg(&net);

    match args.get("output") {
        Some(out) => std::fs::write(out, svg).map_err(|e| format!("write {out}: {e}")),
        None => {
            println!("{svg}");
            Ok(())
        }
    }
}
