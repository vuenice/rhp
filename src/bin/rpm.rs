use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    let rpm_args = if args.len() > 1 { &args[1..] } else { &[] };
    rhp::rpm::run_rpm_cli(rpm_args);
}
