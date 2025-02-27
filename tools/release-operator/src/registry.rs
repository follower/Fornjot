use anyhow::{anyhow, Context};
use secstr::SecStr;
use serde::Deserialize;
use std::fmt::{Display, Formatter};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::str::FromStr;

pub struct Registry {
    token: SecStr,
    crates: Vec<Crate>,
    dry_run: bool,
}

#[derive(Clone, Debug)]
pub struct Crate {
    path: PathBuf,
}

enum CrateState {
    /// Our crate version is ahead of the registry and should be published
    Ahead,
    /// Our crate version is behind the registry; you'll be warned about this
    Behind,
    /// Our crate version matches the registry version
    Published,
    /// We encountered an unknown state while processing the crate
    Unknown,
}

impl Registry {
    pub fn new(token: &SecStr, crates: &[Crate], dry_run: bool) -> Self {
        Self {
            token: token.to_owned(),
            crates: crates.to_vec(),
            dry_run,
        }
    }

    pub fn publish_crates(&self) -> anyhow::Result<()> {
        for c in &self.crates {
            c.validate()?;

            match c.determine_state()? {
                CrateState::Published | CrateState::Behind => continue,
                CrateState::Unknown | CrateState::Ahead => {
                    c.submit(&self.token, self.dry_run)?;
                }
            }
        }

        Ok(())
    }
}

impl Crate {
    fn validate(&self) -> anyhow::Result<()> {
        match self.path.exists() {
            true => Ok(()),
            false => Err(anyhow!(
                "given path to the '{self}' crate is either not readable or does not exist"
            )),
        }
    }

    fn determine_state(&self) -> anyhow::Result<CrateState> {
        let theirs = {
            #[derive(Deserialize)]
            struct CrateVersions {
                versions: Vec<CrateVersion>,
            }

            #[derive(Deserialize)]
            struct CrateVersion {
                #[serde(rename = "num")]
                version: semver::Version,
            }

            let client = reqwest::blocking::ClientBuilder::new()
                .user_agent(concat!(
                    env!("CARGO_PKG_NAME"),
                    "/",
                    env!("CARGO_PKG_VERSION")
                ))
                .build()
                .context("build http client")?;

            let resp = client
                .get(format!("https://crates.io/api/v1/crates/{self}"))
                .send()
                .context("fetching crate versions from the registry")?;

            if resp.status() == reqwest::StatusCode::NOT_FOUND {
                log::info!("{self} has not been published yet");
                return Ok(CrateState::Unknown);
            }

            if resp.status() != reqwest::StatusCode::OK {
                return Err(anyhow!(
                    "{self} request to crates.io failed with {} '{}'",
                    resp.status(),
                    resp.text().unwrap_or_else(|_| {
                        "[response body could not be read]".to_string()
                    })
                ));
            }

            let versions =
                serde_json::from_str::<CrateVersions>(resp.text()?.as_str())
                    .context("deserializing crates.io response")?;

            versions.versions.get(0).unwrap().version.to_owned()
        };

        let ours = {
            let name = format!("{self}");
            let cargo_toml_location = std::fs::canonicalize(&self.path)
                .context("absolute path to Cargo.toml")?;
            let mut cmd = cargo_metadata::MetadataCommand::new();
            cmd.manifest_path(format!(
                "{}/Cargo.toml",
                cargo_toml_location.to_string_lossy()
            ))
            .no_deps();

            let metadata = cmd.exec()?;
            let package = metadata
                .packages
                .iter()
                .find(|p| p.name == name)
                .ok_or_else(|| anyhow!("could not find package"))?;

            let version = package.version.to_owned();
            log::debug!("{self} found as {version} on our side");

            version
        };

        if ours == theirs {
            log::info!("{self} has already been published as {ours}");
            return Ok(CrateState::Published);
        }

        if ours < theirs {
            log::warn!("{self} has already been published as {ours}, which is a newer version");
            return Ok(CrateState::Behind);
        }

        Ok(CrateState::Ahead)
    }

    fn submit(&self, token: &SecStr, dry_run: bool) -> anyhow::Result<()> {
        log::info!("{self} publishing new version");

        std::env::set_current_dir(&self.path)
            .context("switch working directory to the crate in scope")?;

        let cmd = {
            let token = token.to_string();
            let mut cmd = vec!["cargo", "publish", "--token", &token];

            if dry_run {
                cmd.push("--dry-run");
            }

            cmd.join(" ")
        };

        cmd_lib::spawn_with_output!(bash -c $cmd)?.wait_with_pipe(
            &mut |pipe| {
                BufReader::new(pipe)
                    .lines()
                    .flatten()
                    .for_each(|line| println!("{}", line));
            },
        )?;

        Ok(())
    }
}

impl FromStr for Crate {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Crate {
            path: PathBuf::from(s),
        })
    }
}

impl Display for Crate {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(name) = self.path.file_name() {
            return write!(f, "{}", name.to_string_lossy());
        }
        write!(f, "{:?}", self.path)
    }
}
