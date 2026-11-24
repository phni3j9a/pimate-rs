mod agent_cli;
mod alerts;
mod config;
mod github;
mod keeper;
mod launchd;
mod link;
mod login;
mod operations;
mod process;
mod rc;
mod relay;
mod server;
mod service;
mod terminal;
mod update;
mod webhook;
use anyhow::{Result, bail, ensure};
use config::Config;
use nix::ifaddrs::getifaddrs;
use pimate_protocol::{PairingLink, format_pairing_link};
use std::{
    env,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};
fn tail(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.octets()[0] == 100 && (64..=127).contains(&v.octets()[1]),
        IpAddr::V6(v) => v.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    }
}
async fn addresses() -> Result<Vec<IpAddr>> {
    let mut ips = vec![IpAddr::V4(Ipv4Addr::LOCALHOST)];
    for cli in [
        "tailscale",
        "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
    ] {
        let Some(cli) = terminal::executable(std::path::Path::new(cli)) else {
            continue;
        };
        let Ok(Ok(output)) = tokio::time::timeout(
            Duration::from_secs(3),
            tokio::process::Command::new(cli)
                .arg("ip")
                .kill_on_drop(true)
                .output(),
        )
        .await
        else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let local: Vec<_> = getifaddrs()?
            .filter_map(|i| {
                let a = i.address?;
                a.as_sockaddr_in()
                    .map(|v| IpAddr::V4(v.ip()))
                    .or_else(|| a.as_sockaddr_in6().map(|v| IpAddr::V6(v.ip())))
            })
            .collect();
        for ip in String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
        {
            if tail(ip) && local.contains(&ip) && !ips.contains(&ip) {
                ips.push(ip)
            }
        }
        break;
    }
    Ok(ips)
}
fn main() -> std::process::ExitCode {
    let result = if env::args_os().nth(1).is_some_and(|a| a == "__hold") {
        keeper::run()
    } else {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        if env::args_os().nth(1).is_some_and(|a| a == "attach") {
            let result = rt.block_on(async {
                let mut args = env::args().skip(2);
                let name = args.next();
                ensure!(args.next().is_none());
                terminal::Terminal.attach(&config::run_dir(), name).await
            });
            // An attach worker may still be blocked on terminal input or output.
            rt.shutdown_background();
            result
        } else {
            rt.block_on(run()).map(|()| 0)
        }
    };
    result
        .unwrap_or_else(|e| {
            eprintln!("{e}");
            1
        })
        .into()
}
async fn run() -> Result<()> {
    let mut args = env::args().skip(1);
    let cmd = args.next().unwrap_or_default();
    const USAGE: &str = "Usage: pimate serve [--port N] [--listen IP] | pair [--address host:port] [--json] | devices | revoke ID | list | attach [session] | spawn --cwd DIR [-- pi args] | prompt ID TEXT [--steer|--follow-up] [--wait] [--timeout S] | wait ID [--timeout S] | history ID [--last N] | abort ID | shutdown ID | ui ID REQUEST <value|--confirm|--cancel|--answers JSON> | watch ID | webhook <setup|test|status|off|watch|unwatch> | service ACTION | setup-info | pair-pending CODE";
    if matches!(cmd.as_str(), "--help" | "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    if cmd == "webhook" {
        return webhook::cli(args.collect()).await;
    }
    if cmd == "service" {
        let action = args
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing service action"))?;
        ensure!(args.next().is_none());
        println!(
            "{}",
            serde_json::to_string_pretty(&service::run(&action).await?)?
        );
        return Ok(());
    }
    if matches!(
        cmd.as_str(),
        "spawn" | "prompt" | "wait" | "history" | "watch" | "abort" | "shutdown" | "ui"
    ) {
        return agent_cli::run(&cmd, args.collect()).await;
    }
    if cmd == "list" {
        ensure!(args.next().is_none());
        let run = config::run_dir();
        let mut rows = rc::list(&run).await?.instances;
        terminal::Terminal.refresh(&run, &mut rows).await;
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    let config = Config::open(config::config_dir()?)?;
    match cmd.as_str() {
        "setup-info" => {
            ensure!(args.next().is_none());
            println!(
                "{}",
                serde_json::json!({"host":config.host,"configDir":config::config_dir()?,"addresses":addresses().await?})
            )
        }
        "pair-pending" => {
            let code = args.next().ok_or_else(|| anyhow::anyhow!("missing code"))?;
            ensure!(args.next().is_none());
            println!(
                "{}",
                serde_json::json!({"pending":config.pending(&code).try_exists()?})
            )
        }
        "devices" => {
            ensure!(args.next().is_none());
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &config
                        .devices()?
                        .iter()
                        .map(|d| d.public())
                        .collect::<Vec<_>>()
                )?
            )
        }
        "revoke" => {
            let id = args
                .next()
                .ok_or_else(|| anyhow::anyhow!("missing deviceId"))?;
            let as_json = match args.next().as_deref() {
                None => false,
                Some("--json") => true,
                _ => bail!("{USAGE}"),
            };
            ensure!(args.next().is_none());
            config.revoke(&id)?;
            if as_json {
                println!("{{\"ok\":true}}")
            } else {
                println!("Device revoked")
            }
        }
        "pair" | "serve" => {
            let mut port = pimate_protocol::DEFAULT_HOST_PORT;
            let mut address = None;
            let mut listen = vec![];
            let mut as_json = false;
            while let Some(k) = args.next() {
                if cmd == "pair" && k == "--json" {
                    as_json = true;
                    continue;
                }
                let v = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("missing value"))?;
                match k.as_str() {
                    "--port" => port = v.parse()?,
                    "--address" if cmd == "pair" => address = Some(v),
                    "--listen" if cmd == "serve" => listen.push(v.parse::<IpAddr>()?),
                    _ => bail!("unexpected option {k}"),
                }
            }
            let ips = addresses().await?;
            if ips.len() == 1 {
                eprintln!("No local Tailscale address found; default is loopback only.")
            }
            if cmd == "serve" {
                ensure!(
                    listen.iter().all(|ip| ips.contains(ip)),
                    "not loopback or a local Tailscale address"
                );
                server::serve(config, if listen.is_empty() { ips } else { listen }, port).await?
            } else {
                let address = address.unwrap_or_else(|| {
                    SocketAddr::new(*ips.get(1).unwrap_or(&ips[0]), port).to_string()
                });
                ensure!(port != 0, "invalid pairing port");
                ensure!(
                    pimate_protocol::valid_pairing_address(&address)
                        && address
                            .rsplit_once(':')
                            .and_then(|(_, p)| p.parse::<u16>().ok())
                            .is_some_and(|p| p > 0),
                    "invalid address"
                );
                let (code, expires_at) = config.code()?;
                let link = format_pairing_link(&PairingLink {
                    address,
                    code: code.clone(),
                    host_name: config.host.name,
                    host_id: config.host.host_id,
                });
                if as_json {
                    println!(
                        "{}",
                        serde_json::json!({"code":code,"link":link,"expiresAt":expires_at})
                    );
                    return Ok(());
                }
                println!(
                    "{link}\n{}",
                    qrcode::QrCode::new(&link)?
                        .render::<qrcode::render::unicode::Dense1x2>()
                        .build()
                )
            }
        }
        _ => bail!("{USAGE}"),
    };
    Ok(())
}
