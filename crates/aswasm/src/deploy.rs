use crate::image::Image;
use crate::runtime::Allocator;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::ops::Not;
use std::path::{Path, PathBuf};
use wasmtime::Instance;
use ya_runtime_api::deploy::{ContainerVolume, DeployResult, StartMode};

pub const MANIFEST_FILE: &str = "manifest.json";

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct Manifest {
    pub id: String,
    pub name: String,
    pub runtime: String,

    #[serde(default)]
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub entry_points: HashMap<String, EntryPoint>,

    pub main: MainEntry,

    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mount_points: Vec<MountPoint>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum MountPoint {
    Ro(String),
    Rw(String),
    Wo(String),
    Private(String),
}

impl MountPoint {
    pub fn path(&self) -> &str {
        match self {
            Self::Ro(p) | Self::Rw(p) | Self::Wo(p) | Self::Private(p) => p.as_str(),
        }
    }

    pub fn is_public(&self) -> bool {
        matches!(self, Self::Private(_)).not()
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct MainEntry {
    pub wasm_path: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct EntryPoint {
    pub desc: Option<String>,
    pub args: Vec<ArgDesc>,
    #[serde(default)]
    pub output: Output,
}

impl EntryPoint {
    pub fn convert_args(
        &self,
        instance: &Instance,
        args: Vec<String>,
    ) -> anyhow::Result<Vec<wasmtime::Val>> {
        args.into_iter()
            .zip(self.args.iter())
            .map(|(str_val, arg_desc)| arg_desc.convert_arg(instance, str_val))
            .collect::<anyhow::Result<Vec<_>>>()
            .with_context(|| format!("converting args {:?}", self.args))
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct ArgDesc {
    name: Option<String>,
    #[serde(flatten)]
    arg_type: ArgType,
}

impl ArgDesc {
    fn convert_arg(&self, instance: &Instance, arg: String) -> anyhow::Result<wasmtime::Val> {
        let mut a = Allocator::for_instance(instance)?;
        match &self.arg_type {
            ArgType::String {} => {
                // TODO: retain/release
                let ptr = a.new_string(&arg)?;
                Ok(wasmtime::Val::from(ptr))
            }
            ArgType::Bytes { fixed: _ } => {
                let data = hex::decode(arg)?;
                // TODO: retain/release
                let ptr = a.new_bytes(&data)?;
                Ok(wasmtime::Val::from(ptr))
            }
            ArgType::I32 => {
                let v: i32 = arg.parse()?;
                Ok(wasmtime::Val::from(v))
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
#[serde(tag = "type")]
pub enum ArgType {
    String {},
    Bytes { fixed: Option<usize> },
    I32,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum Output {
    ExitCode,
    Bytes,
    String,
    Void,
}

impl Default for Output {
    fn default() -> Self {
        Self::ExitCode
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Deployment {
    entry_points: HashMap<String, EntryPoint>,
    image_path: PathBuf,
    main: MainEntry,
    vols: Vec<ContainerVolume>,
}

impl Deployment {
    pub fn save(&self, work_dir: &Path) -> anyhow::Result<()> {
        let deploy_file = work_dir.join("deploy.json");
        serde_json::to_writer_pretty(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(deploy_file)?,
            self,
        )?;
        Ok(())
    }

    pub fn load(work_dir: &Path) -> anyhow::Result<Self> {
        let deploy_file = work_dir.join("deploy.json");
        Ok(serde_json::from_slice(&std::fs::read(deploy_file)?)?)
    }

    pub fn get_image(&self) -> anyhow::Result<Image<File>> {
        Ok(Image::from_path(&self.image_path)?)
    }

    #[inline]
    pub fn main_entry(&self) -> &MainEntry {
        &self.main
    }

    pub fn entry_point(&self, name: &str) -> Option<&EntryPoint> {
        self.entry_points.get(name)
    }

    pub fn vols(&self) -> Vec<ContainerVolume> {
        self.vols
            .iter()
            .map(|v| ContainerVolume {
                name: v.name.clone(),
                path: v.path.clone(),
            })
            .collect()
    }
}

pub fn deploy(workdir: &Path, path: &Path) -> anyhow::Result<DeployResult> {
    let mut image = Image::from_path(path)?;
    let manifest: Manifest = image.get_json(MANIFEST_FILE)?;

    let mut vols = Vec::new();
    let mut public_vols = Vec::new();
    for mount_point in manifest.mount_points {
        let name = format!("vol-{}", uuid::Uuid::new_v4());
        let dir = workdir.join(&name);
        std::fs::create_dir_all(dir)?;
        vols.push(ContainerVolume {
            name: name.clone(),
            path: mount_point.path().to_string(),
        });
        if mount_point.is_public() {
            public_vols.push(ContainerVolume {
                name,
                path: mount_point.path().to_string(),
            })
        }
    }
    let deployment = Deployment {
        entry_points: manifest.entry_points,
        main: manifest.main,
        image_path: path.to_path_buf(),
        vols,
    };

    deployment.save(workdir)?;

    Ok(DeployResult {
        valid: Ok("valid".to_string()),
        vols: public_vols,
        start_mode: StartMode::Blocking,
    })
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse() {
        let json = r#"{
  "id": "ce38dba2-19ce-11eb-a060-57e8812ec8da",
  "name": "MyApp",
  "runtime": "aswasm",
  "main": {
    "wasm-path": "app.wasm"
  },
  "entry-points": {
    "init": {
      "args": [
        {
          "name": "contract",
          "type": "bytes"
        },
        {
          "name": "voting_id",
          "type": "string"
        }
      ]
    },
    "register": {
      "output": "bytes",
      "args": [
        {
          "name": "contract",
          "type": "bytes"
        },
        {
          "name": "voting_id",
          "type": "string"
        },
        {
          "name": "operator_addr",
          "type": "bytes"
        },
        {
          "name": "sender",
          "type": "bytes",
          "fixed": 20
        },
        {
          "name": "signature",
          "type": "bytes"
        },
        {
          "name": "session_pub_key",
          "type": "bytes"
        }
      ]
    }
  }
}"#;
        let m: Manifest = serde_json::from_str(json).unwrap();
        eprintln!("{:?}", m)
    }
}
