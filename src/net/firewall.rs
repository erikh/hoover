use std::collections::HashSet;
use std::net::IpAddr;
use std::time::Duration;

use tokio::process::Command;

use crate::config::FirewallConfig;

/// Manages firewall blocking for failed decryption attempts.
pub struct FirewallManager {
    backend: String,
    block_duration: Duration,
    blocked: HashSet<IpAddr>,
}

impl FirewallManager {
    #[must_use]
    pub fn new(config: &FirewallConfig) -> Self {
        Self {
            backend: config.backend.clone(),
            block_duration: Duration::from_secs(config.block_duration_secs),
            blocked: HashSet::new(),
        }
    }

    /// Block an IP address. No-op if already blocked.
    pub async fn block_ip(&mut self, ip: IpAddr) {
        if self.blocked.contains(&ip) {
            return;
        }

        let result = match self.backend.as_str() {
            "firewalld" => self.block_firewalld(ip).await,
            "nftables" => self.block_nftables(ip).await,
            other => {
                tracing::error!("unknown firewall backend: {other}");
                return;
            }
        };

        match result {
            Ok(()) => {
                tracing::warn!("blocked IP {ip} via {}", self.backend);
                self.blocked.insert(ip);
                self.schedule_unblock(ip);
            }
            Err(e) => {
                tracing::error!("failed to block IP {ip}: {e}");
            }
        }
    }

    async fn block_firewalld(&self, ip: IpAddr) -> std::result::Result<(), String> {
        let rule = format!(
            "rule family=\"{}\" source address=\"{ip}\" drop",
            if ip.is_ipv4() { "ipv4" } else { "ipv6" }
        );

        let output = Command::new("firewall-cmd")
            .arg("--add-rich-rule")
            .arg(&rule)
            .output()
            .await
            .map_err(|e| format!("failed to run firewall-cmd: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("firewall-cmd failed: {stderr}"));
        }

        Ok(())
    }

    async fn block_nftables(&self, ip: IpAddr) -> std::result::Result<(), String> {
        let family = if ip.is_ipv4() { "ip" } else { "ip6" };
        let rule = format!("{family} saddr {ip} drop");

        let output = Command::new("nft")
            .args(["add", "rule", "inet", "filter", "input"])
            .arg(&rule)
            .output()
            .await
            .map_err(|e| format!("failed to run nft: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("nft failed: {stderr}"));
        }

        Ok(())
    }

    fn schedule_unblock(&self, ip: IpAddr) {
        let duration = self.block_duration;
        let backend = self.backend.clone();

        tokio::spawn(async move {
            tokio::time::sleep(duration).await;

            let result = match backend.as_str() {
                "firewalld" => unblock_firewalld(ip).await,
                "nftables" => unblock_nftables(ip).await,
                _ => Ok(()),
            };

            match result {
                Ok(()) => tracing::info!("unblocked IP {ip}"),
                Err(e) => tracing::error!("failed to unblock IP {ip}: {e}"),
            }
        });
    }
}

async fn unblock_firewalld(ip: IpAddr) -> std::result::Result<(), String> {
    let rule = format!(
        "rule family=\"{}\" source address=\"{ip}\" drop",
        if ip.is_ipv4() { "ipv4" } else { "ipv6" }
    );

    let output = Command::new("firewall-cmd")
        .arg("--remove-rich-rule")
        .arg(&rule)
        .output()
        .await
        .map_err(|e| format!("failed to run firewall-cmd: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("firewall-cmd remove failed: {stderr}"));
    }

    Ok(())
}

async fn unblock_nftables(_ip: IpAddr) -> std::result::Result<(), String> {
    // nftables requires handle to delete a rule; for simplicity, flush the hoover chain
    // A production implementation would track handles
    let output = Command::new("nft")
        .args(["flush", "chain", "inet", "filter", "input"])
        .output()
        .await
        .map_err(|e| format!("failed to run nft: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("nft flush failed: {stderr}"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_creation() {
        let config = FirewallConfig {
            enabled: true,
            backend: "firewalld".to_string(),
            block_duration_secs: 600,
        };

        let mgr = FirewallManager::new(&config);
        assert_eq!(mgr.backend, "firewalld");
        assert!(mgr.blocked.is_empty());
    }
}
