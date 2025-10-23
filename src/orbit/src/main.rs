use clap::{Parser, Subcommand};
use zbus::{Result, blocking::Connection, proxy};

#[derive(Parser, Debug)]
#[command(name = "orbit", version)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Reload,
    Modules,
    Toggle {
        #[arg(help = "Module name to toggle")]
        module: String,
    },
    Exit,
}

#[proxy(
    interface = "io.github.orbitshell.Orbit1",
    default_service = "io.github.orbitshell.Orbit1",
    default_path = "/io/github/orbitshell/Orbit1",
    gen_blocking = true,
    gen_async = false,
    blocking_name = "OrbitProxy"
)]
trait Orbit {
    fn alive(&self) -> Result<()>;
    fn reload(&self) -> Result<String>;
    fn modules(&self) -> Result<String>;
    fn toggle(&self, module: &str) -> Result<()>;
    fn exit(&self) -> Result<()>;
}

fn main() {
    let args = Args::parse();

    let Ok((conn, proxy)) = (|| {
        let conn = Connection::session()?;
        let proxy = OrbitProxy::new(&conn)?;
        proxy.alive()?;
        Ok::<_, zbus::Error>((conn, proxy))
    })() else {
        eprintln!("Orbit is not running.");
        return;
    };

    let _conn = conn;

    match args.command {
        Commands::Reload => match proxy.reload() {
            Ok(m) => println!("{m}"),
            Err(e) => eprintln!("Reload failed: {e}"),
        },
        Commands::Modules => match proxy.modules() {
            Ok(m) => println!("{m}"),
            Err(e) => eprintln!("Modules failed: {e}"),
        },
        Commands::Toggle { module } => {
            if let Err(e) = proxy.toggle(&module) {
                eprintln!("Toggle failed: {e}");
            }
        }
        Commands::Exit => {
            if let Err(e) = proxy.exit() {
                eprintln!("Exit failed: {e}");
            }
        }
    }
}
