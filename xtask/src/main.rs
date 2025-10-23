use std::{env, ffi::OsString, fs, path::PathBuf, process::Command};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (profile, package, pass_through_args) = parse_args()?;

    build_workspace(&profile)?;

    let target_dir = project_root().join("target").join(&profile);
    let modules_dir = project_root().join("modules");
    let config_modules_dir = xdg_config_home().join("modules");

    fs::create_dir_all(&config_modules_dir)?;

    println!("Copying modules to {:?}", config_modules_dir);

    for entry in fs::read_dir(&modules_dir)? {
        let entry = entry?;
        let module_path = entry.path();

        if module_path.is_dir() {
            let module_name = module_path
                .file_name()
                .expect("module name missing")
                .to_string_lossy();

            let so_src = target_dir.join(format!("lib{module_name}.so"));

            let so_dst = config_modules_dir.join(format!("{module_name}.so"));

            println!("> Copying {so_src:?} -> {so_dst:?}");

            fs::copy(&so_src, &so_dst)?;
        }
    }

    if let Some(pkg) = package.as_deref() {
        run_package(pkg, &pass_through_args)?;
    }
    Ok(())
}

type Args = (String, Option<String>, Vec<OsString>);

fn parse_args() -> Result<Args, Box<dyn std::error::Error>> {
    let mut profile = String::from("release");
    let mut package: Option<String> = None;
    let mut pass_through: Vec<OsString> = Vec::new();

    let mut args = env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--" {
            pass_through.extend(args);
            break;
        } else if arg == "--profile" {
            let val = args.next().ok_or("--profile requires a value")?;
            profile = val.to_string_lossy().into_owned();
        } else if arg == "--package" || arg == "-p" {
            let val = args.next().ok_or("--package/-p requires a value")?;
            package = Some(val.to_string_lossy().into_owned());
        } else if let Some(s) = arg.to_str() {
            if s.starts_with('-') {
                eprintln!("(warning) unrecognized flag: {s}");
            } else if package.is_none() {
                package = Some(s.to_string());
            } else {
                pass_through.push(arg);
            }
        } else {
            pass_through.push(arg);
        }
    }

    Ok((profile, package, pass_through))
}

fn build_workspace(profile: &str) -> Result<(), Box<dyn std::error::Error>> {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());

    let status = Command::new(cargo)
        .current_dir(project_root())
        .args(["build", "--workspace", "--profile", profile])
        .status()?;

    if !status.success() {
        Err("cargo build failed")?;
    }

    Ok(())
}

fn run_package(package: &str, pass_args: &[OsString]) -> Result<(), Box<dyn std::error::Error>> {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());

    let status = Command::new(cargo)
        .current_dir(project_root())
        .args(["run", "-p", package, "--"])
        .args(pass_args)
        .status()?;

    if !status.success() {
        Err("cargo run failed")?;
    }

    Ok(())
}

fn xdg_config_home() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let mut home = env::var_os("HOME")
                .map(PathBuf::from)
                .expect("HOME not set");
            home.push(".config");
            home
        })
        .join("orbit")
}

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}
