mod cli;
mod list;
#[allow(dead_code)]
mod serial;

use clap::Parser;
use cli::Cli;

fn main() {
    let args = Cli::parse();

    if args.list {
        cmd_list(&args);
        return;
    }

    if args.json {
        cmd_json(&args);
        return;
    }

    if args.send.is_some() {
        cmd_send_expect(&args);
        return;
    }

    if let Some(cli::Commands::Mcp) = &args.command {
        cmd_mcp(&args);
        return;
    }

    cmd_interactive(&args);
}

fn cmd_list(args: &Cli) {
    let devices = list::enumerate_devices();
    if args.json {
        print!("{}", list::render_json(&devices));
    } else {
        print!("{}", list::render_table(&devices));
    }
}

fn cmd_json(_args: &Cli) {
    eprintln!("not implemented");
    std::process::exit(1);
}

fn cmd_send_expect(_args: &Cli) {
    eprintln!("not implemented");
    std::process::exit(1);
}

fn cmd_mcp(_args: &Cli) {
    eprintln!("not implemented");
    std::process::exit(1);
}

fn cmd_interactive(_args: &Cli) {
    eprintln!("not implemented");
    std::process::exit(1);
}
