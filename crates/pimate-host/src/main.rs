mod config;
mod link;
mod rc;
mod relay;
mod server;
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
#[tokio::main]
async fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let cmd = args.next().unwrap_or_default();
    if cmd == "list" {
        ensure!(args.next().is_none());
        println!(
            "{}",
            serde_json::to_string_pretty(&rc::list(&config::run_dir()).await?.instances)?
        );
        return Ok(());
    }
    let config = Config::open(config::config_dir()?)?;
    match cmd.as_str() {
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
            ensure!(args.next().is_none());
            config.revoke(&id)?
        }
        "pair" | "serve" => {
            let mut port = pimate_protocol::DEFAULT_HOST_PORT;
            let mut address = None;
            let mut listen = vec![];
            while let Some(k) = args.next() {
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
                let link = format_pairing_link(&PairingLink {
                    address,
                    code: config.code()?,
                    host_name: config.host.name,
                    host_id: config.host.host_id,
                });
                println!(
                    "{link}\n{}",
                    qrcode::QrCode::new(&link)?
                        .render::<qrcode::render::unicode::Dense1x2>()
                        .build()
                )
            }
        }
        _ => bail!(
            "Usage: pimate serve [--port N] [--listen IP] | pair [--address host:port] | devices | revoke ID | list"
        ),
    };
    Ok(())
}
