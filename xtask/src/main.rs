use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Copy)]
struct BinaryPackage {
    target: &'static str,
    asset_name: &'static str,
    binary_name: &'static str,
    archive_kind: ArchiveKind,
}

#[derive(Clone, Copy)]
enum ArchiveKind {
    TarGz,
    Zip,
}

const PACKAGES: &[BinaryPackage] = &[
    BinaryPackage {
        target: "x86_64-apple-darwin",
        asset_name: "posterm-macos-x86_64.tar.gz",
        binary_name: "posterm",
        archive_kind: ArchiveKind::TarGz,
    },
    BinaryPackage {
        target: "aarch64-apple-darwin",
        asset_name: "posterm-macos-aarch64.tar.gz",
        binary_name: "posterm",
        archive_kind: ArchiveKind::TarGz,
    },
    BinaryPackage {
        target: "x86_64-unknown-linux-gnu",
        asset_name: "posterm-linux-x86_64.tar.gz",
        binary_name: "posterm",
        archive_kind: ArchiveKind::TarGz,
    },
    BinaryPackage {
        target: "aarch64-unknown-linux-gnu",
        asset_name: "posterm-linux-aarch64.tar.gz",
        binary_name: "posterm",
        archive_kind: ArchiveKind::TarGz,
    },
    BinaryPackage {
        target: "x86_64-pc-windows-gnu",
        asset_name: "posterm-windows-x86_64.zip",
        binary_name: "posterm.exe",
        archive_kind: ArchiveKind::Zip,
    },
];

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        None | Some("build-releases") => build_releases(),
        Some("--help") | Some("-h") => {
            println!("Usage: cargo build-releases");
            Ok(())
        }
        Some(other) => Err(format!("unknown xtask command: {other}").into()),
    }
}

fn build_releases() -> Result<()> {
    let repo = repo_root()?;
    let dist = repo.join("dist");

    if dist.exists() {
        fs::remove_dir_all(&dist)?;
    }
    fs::create_dir_all(&dist)?;

    for package in PACKAGES {
        run(Command::new("cargo")
            .current_dir(&repo)
            .arg("build")
            .arg("--release")
            .arg("--target")
            .arg(package.target))?;

        let binary = repo
            .join("target")
            .join(package.target)
            .join("release")
            .join(package.binary_name);
        if !binary.is_file() {
            return Err(format!("expected binary was not created: {}", binary.display()).into());
        }

        let archive = dist.join(package.asset_name);
        match package.archive_kind {
            ArchiveKind::TarGz => create_tar_gz(&archive, &binary, package.binary_name)?,
            ArchiveKind::Zip => create_zip(&archive, &binary, package.binary_name)?,
        }
    }

    write_checksums(&dist)?;

    println!("Release artifacts written to {}", dist.display());
    Ok(())
}

fn repo_root() -> Result<PathBuf> {
    let output = Command::new("git")
        .arg("rev-parse")
        .arg("--show-toplevel")
        .stderr(Stdio::inherit())
        .output()?;

    if !output.status.success() {
        return Err("failed to locate git repository root".into());
    }

    let path = String::from_utf8(output.stdout)?;
    Ok(PathBuf::from(path.trim()))
}

fn create_tar_gz(archive: &Path, binary: &Path, name: &str) -> Result<()> {
    run(Command::new("tar")
        .arg("-czf")
        .arg(archive)
        .arg("-C")
        .arg(
            binary
                .parent()
                .ok_or_else(|| format!("binary has no parent: {}", binary.display()))?,
        )
        .arg(name))
}

fn create_zip(archive: &Path, binary: &Path, name: &str) -> Result<()> {
    run(Command::new("zip")
        .arg("-j")
        .arg("-q")
        .arg(archive)
        .arg(binary))?;
    assert_zip_member_name(archive, name)
}

fn assert_zip_member_name(archive: &Path, expected: &str) -> Result<()> {
    let output = Command::new("zipinfo").arg("-1").arg(archive).output()?;
    if !output.status.success() {
        return Err(format!("failed to inspect zip archive: {}", archive.display()).into());
    }

    let members = String::from_utf8(output.stdout)?;
    let found = members.lines().any(|line| line == expected);
    if !found {
        return Err(format!(
            "zip archive {} did not contain expected member {expected}",
            archive.display()
        )
        .into());
    }
    Ok(())
}

fn write_checksums(dist: &Path) -> Result<()> {
    let mut artifacts = Vec::new();
    for entry in fs::read_dir(dist)? {
        let path = entry?.path();
        if path.is_file()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| !name.ends_with(".sha256") && name != "checksums.txt")
        {
            artifacts.push(path);
        }
    }
    artifacts.sort();

    let mut aggregate = String::new();
    for artifact in artifacts {
        let output = Command::new("shasum")
            .arg("-a")
            .arg("256")
            .arg(&artifact)
            .output()?;
        if !output.status.success() {
            return Err(format!("failed to checksum {}", artifact.display()).into());
        }

        let line = String::from_utf8(output.stdout)?;
        let hash = line
            .split_whitespace()
            .next()
            .ok_or_else(|| format!("empty checksum output for {}", artifact.display()))?;
        let file_name = artifact
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("invalid artifact name: {}", artifact.display()))?;
        let checksum_line = format!("{hash}  {file_name}\n");

        fs::write(
            artifact.with_file_name(format!("{file_name}.sha256")),
            &checksum_line,
        )?;
        aggregate.push_str(&checksum_line);
    }

    fs::write(dist.join("checksums.txt"), aggregate)?;
    Ok(())
}

fn run(command: &mut Command) -> Result<()> {
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(command_error(command, status).into())
    }
}

fn command_error(command: &Command, status: std::process::ExitStatus) -> String {
    format!("command failed with {status}: {}", display_command(command))
}

fn display_command(command: &Command) -> String {
    let mut parts = Vec::new();
    parts.push(command.get_program().to_string_lossy().into_owned());
    parts.extend(
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned()),
    );
    parts.join(" ")
}
