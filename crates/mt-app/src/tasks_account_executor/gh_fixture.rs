// Standalone synthetic gh executable. Compiled and executed by Actions tests only.
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PREFIX: &str = "fixture_credential_";

fn argument<'a>(args: &'a [String], key: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == key)
        .map(|pair| pair[1].as_str())
}

fn descendant(inherit_pipes: bool) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.arg("--descendant").stdin(Stdio::null());
    if !inherit_pipes {
        command.stdout(Stdio::null()).stderr(Stdio::null());
    }
    command.spawn().unwrap();
    let ready = evidence_path("MT_FIXTURE_READY", "ready");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready.exists() {
        assert!(Instant::now() < deadline, "descendant did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn evidence_path(variable: &str, name: &str) -> std::path::PathBuf {
    std::env::var_os(variable)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap().join(name))
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--descendant") {
        let release = evidence_path("MT_FIXTURE_RELEASE", "release");
        std::fs::write(evidence_path("MT_FIXTURE_READY", "ready"), b"started").unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !release.exists() {
            if Instant::now() >= deadline {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        std::fs::write(evidence_path("MT_FIXTURE_MARKER", "marker"), b"orphan").unwrap();
        return;
    }
    if args.first().map(String::as_str) == Some("--suspended") {
        std::fs::write(std::env::var_os("MT_FIXTURE_MARKER").unwrap(), b"resumed").unwrap();
        return;
    }
    assert!(!args.iter().any(|arg| arg.contains(PREFIX)));
    for key in [
        "GH_DEBUG",
        "DEBUG",
        "GH_HOST",
        "GH_REPO",
        "GH_FORCE_TTY",
        "BASH_ENV",
        "ENV",
        "WSLENV",
    ] {
        assert!(
            std::env::var_os(key).is_none(),
            "unexpected inherited override"
        );
    }
    if args.iter().any(|arg| arg == "--help") {
        if std::env::var_os("MT_FIXTURE_UNSUPPORTED").is_some() {
            println!("FLAGS\n  -h, --hostname string Host");
            return;
        }
        println!(
            "FLAGS\n  -h, --hostname string Host\n      --json fields JSON\n  -u, --user string Account"
        );
        return;
    }
    if args.starts_with(&["auth".into(), "status".into()]) {
        for key in [
            "GH_TOKEN",
            "GITHUB_TOKEN",
            "GH_ENTERPRISE_TOKEN",
            "GITHUB_ENTERPRISE_TOKEN",
        ] {
            assert!(std::env::var_os(key).is_none());
        }
        let host = argument(&args, "--hostname").unwrap();
        match std::env::var("MT_FIXTURE_ENUM_CASE").as_deref() {
            Ok("none") => {
                println!("{{\"hosts\":{{}}}}");
                return;
            }
            Ok("unsupported") => {
                eprintln!("unknown flag: --json {PREFIX}hidden");
                std::process::exit(1);
            }
            Ok("malformed") => {
                println!("{{{PREFIX}hidden");
                return;
            }
            _ => {}
        }
        let problem =
            std::env::var("MT_FIXTURE_ENUM_ERROR").unwrap_or_else(|_| "Bad credentials".into());
        println!(
            "{{\"token\":\"{PREFIX}extra\",\"hosts\":{{\"{host}\":[{{\"state\":\"success\",\"active\":true,\"host\":\"{host}\",\"login\":\"Alice\",\"tokenSource\":\"keyring\",\"token\":\"{PREFIX}extra\"}},{{\"state\":\"error\",\"error\":\"{problem}; {PREFIX}hidden\",\"active\":false,\"host\":\"{host}\",\"login\":\"Broken\",\"tokenSource\":\"keyring\"}}]}}}}"
        );
        return;
    }
    if args.starts_with(&["auth".into(), "token".into()]) {
        for key in [
            "GH_TOKEN",
            "GITHUB_TOKEN",
            "GH_ENTERPRISE_TOKEN",
            "GITHUB_ENTERPRISE_TOKEN",
        ] {
            assert!(std::env::var_os(key).is_none());
        }
        let user = argument(&args, "--user").unwrap();
        if user == "LookupSlow" {
            descendant(true);
            println!("{PREFIX}{user}");
            std::io::stdout().flush().unwrap();
            std::thread::sleep(Duration::from_secs(30));
            return;
        }
        if user == "Broken" {
            println!("{PREFIX}Broken");
            eprintln!("no oauth token found; {PREFIX}Broken");
            std::process::exit(1);
        }
        if user == "Store" {
            eprintln!("keyring is locked; {PREFIX}Store");
            std::process::exit(1);
        }
        let user = if user == "Rotate" && evidence_path("MT_FIXTURE_PHASE", "data-seen").exists() {
            "Replacement"
        } else {
            user
        };
        println!("{PREFIX}{user}");
        return;
    }
    let token = std::env::var("GH_TOKEN")
        .or_else(|_| std::env::var("GH_ENTERPRISE_TOKEN"))
        .unwrap();
    assert!(std::env::var_os("GITHUB_TOKEN").is_none());
    assert!(std::env::var_os("GITHUB_ENTERPRISE_TOKEN").is_none());
    let user = token.strip_prefix(PREFIX).unwrap();
    let host = argument(&args, "--hostname")
        .or_else(|| argument(&args, "--repo").map(|repo| repo.split('/').next().unwrap()))
        .unwrap();
    let variable = if host == "github.com" || host.ends_with(".ghe.com") {
        "GH_TOKEN"
    } else {
        "GH_ENTERPRISE_TOKEN"
    };
    assert_eq!(std::env::var(variable).unwrap(), token);
    if args.first().map(String::as_str) == Some("api") {
        let after_data = evidence_path("MT_FIXTURE_PHASE", "data-seen").exists();
        let login = if user == "Wrong" || (user == "WrongAfter" && after_data) {
            "Other"
        } else {
            user
        };
        println!("{{\"login\":\"{login}\"}}");
        return;
    }
    if user == "Leak" || user == "LeakStderr" {
        if user == "Leak" {
            println!("{{\"secret\":\"{token}\"}}");
        } else {
            println!("[]");
        }
        eprintln!("{token}");
        return;
    }
    if user == "Rotate" || user == "WrongAfter" {
        std::fs::write(evidence_path("MT_FIXTURE_PHASE", "data-seen"), b"data-read").unwrap();
    }
    if user == "Large" {
        std::io::stdout()
            .write_all(&vec![b'x'; 3 * 1024 * 1024])
            .unwrap();
        return;
    }
    if matches!(user, "Slow" | "Descendant" | "PipeDescendant") {
        descendant(user == "PipeDescendant");
        if user == "Slow" {
            let finish = std::env::current_dir().unwrap().join("finish");
            let deadline = Instant::now() + Duration::from_secs(30);
            while !finish.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
    let detail = args.get(1).map(String::as_str) == Some("view");
    let number = if detail { &args[2] } else { "1" };
    let item = format!(
        "{{\"number\":{number},\"title\":\"{user}\",\"state\":\"OPEN\",\"author\":{{\"login\":\"{user}\"}},\"labels\":[],\"updatedAt\":\"2026-09-06T01:02:03Z\",\"url\":\"https://{host}/owner/repo/issues/{number}\",\"body\":\"{user}\"}}"
    );
    if detail {
        println!("{item}");
    } else {
        println!("[{item}]");
    }
}
