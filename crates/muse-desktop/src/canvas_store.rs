//! An index plus one graph per canvas; the legacy singleton is kept as a backup.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
use uuid::Uuid;

const INDEX_FILE: &str = "canvas-index.json";
const LEGACY_FILE: &str = "canvas-graph.json";
const MAX_GRAPH_BYTES: usize = 8_000_000;

#[derive(Clone, Serialize, Deserialize)]
pub struct CanvasInfo {
    pub id: Uuid,
    pub name: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CanvasLibrary {
    version: u8,
    pub active: Uuid,
    pub canvases: Vec<CanvasInfo>,
}

impl CanvasLibrary {
    pub fn load(root: &Path) -> Result<Self, String> {
        let index = root.join(INDEX_FILE);
        if index.exists() {
            let value = read_json(&index, 1_000_000)?;
            let library: Self =
                serde_json::from_value(value).map_err(|_| "画布列表损坏，已停止写入")?;
            let mut ids = HashSet::new();
            if library.version != 1
                || library.canvases.is_empty()
                || library.canvases.len() > 500
                || library
                    .canvases
                    .iter()
                    .any(|c| c.id.is_nil() || !ids.insert(c.id) || checked_name(&c.name).is_err())
                || !ids.contains(&library.active)
            {
                return Err("画布列表格式不受支持，已保留原文件".into());
            }
            library.graph(root, library.active)?;
            return Ok(library);
        }
        let legacy = root.join(LEGACY_FILE);
        let graph = if legacy.exists() {
            read_json(&legacy, MAX_GRAPH_BYTES)?
        } else {
            empty_graph()
        };
        validate_graph(&graph)?;
        let id = Uuid::new_v4();
        let library = Self {
            version: 1,
            active: id,
            canvases: vec![CanvasInfo {
                id,
                name: if legacy.exists() {
                    "我的画布"
                } else {
                    "未命名画布"
                }
                .into(),
            }],
        };
        std::fs::create_dir_all(root.join("canvases")).map_err(|_| "无法创建画布目录")?;
        library.save_graph(root, id, &graph)?;
        write_json(
            &index,
            &serde_json::to_value(&library).map_err(|_| "无法保存画布列表")?,
        )?;
        Ok(library)
    }

    pub fn info(&self, id: Uuid) -> Result<&CanvasInfo, String> {
        self.canvases
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| "画布不存在，请刷新列表".into())
    }

    pub fn graph(&self, root: &Path, id: Uuid) -> Result<Value, String> {
        self.info(id)?;
        let graph = read_json(&graph_path(root, id), MAX_GRAPH_BYTES)?;
        validate_graph(&graph)?;
        Ok(graph)
    }

    pub fn save_graph(&self, root: &Path, id: Uuid, graph: &Value) -> Result<(), String> {
        self.info(id)?;
        validate_graph(graph)?;
        write_json(&graph_path(root, id), graph)
    }

    pub fn create(&mut self, root: &Path, name: &str) -> Result<(), String> {
        if self.canvases.len() >= 500 {
            return Err("画布数量已达上限，请先整理旧画布".into());
        }
        let name = checked_name(name)?;
        let mut next = self.clone();
        let id = Uuid::new_v4();
        next.canvases.push(CanvasInfo { id, name });
        next.active = id;
        next.save_graph(root, id, &empty_graph())?;
        self.commit(root, next)
    }

    pub fn select(&mut self, root: &Path, id: Uuid) -> Result<(), String> {
        self.graph(root, id)?;
        let mut next = self.clone();
        next.active = id;
        self.commit(root, next)
    }

    pub fn rename(&mut self, root: &Path, id: Uuid, name: &str) -> Result<(), String> {
        self.info(id)?;
        let name = checked_name(name)?;
        let mut next = self.clone();
        next.canvases
            .iter_mut()
            .find(|c| c.id == id)
            .expect("checked canvas")
            .name = name;
        self.commit(root, next)
    }

    pub fn remove(&mut self, root: &Path, id: Uuid) -> Result<(), String> {
        self.info(id)?;
        let mut next = self.clone();
        next.canvases.retain(|c| c.id != id);
        if next.canvases.is_empty() {
            let new_id = Uuid::new_v4();
            next.canvases.push(CanvasInfo {
                id: new_id,
                name: "未命名画布".into(),
            });
            next.save_graph(root, new_id, &empty_graph())?;
        }
        if next.active == id {
            next.active = next.canvases[0].id;
        }
        self.commit(root, next)?;
        // Shared media and studio history are deliberately retained.
        std::fs::remove_file(graph_path(root, id))
            .map_err(|_| "画布已从列表移除，但旧文件清理失败".into())
    }

    fn commit(&mut self, root: &Path, next: Self) -> Result<(), String> {
        write_json(
            &root.join(INDEX_FILE),
            &serde_json::to_value(&next).map_err(|_| "无法保存画布列表")?,
        )?;
        *self = next;
        Ok(())
    }

    pub fn complete_task(
        &self,
        root: &Path,
        canvas: Uuid,
        node: &str,
        request: &str,
        url: Option<&str>,
        status: &str,
    ) -> Result<(), String> {
        let mut graph = self.graph(root, canvas)?;
        let Some(node) = graph["nodes"].as_array_mut().and_then(|nodes| {
            nodes.iter_mut().find(|n| {
                n["id"].as_str() == Some(node) && n["requestId"].as_str() == Some(request)
            })
        }) else {
            return Ok(());
        };
        if let Some(url) = url {
            node["outputUrl"] = json!(url);
        }
        node["isGenerating"] = json!(false);
        node["lastMeta"] = json!(status);
        node["error"] = json!(if url.is_some() { "" } else { status });
        self.save_graph(root, canvas, &graph)
    }
}

fn graph_path(root: &Path, id: Uuid) -> PathBuf {
    root.join("canvases").join(format!("{id}.json"))
}
fn empty_graph() -> Value {
    json!({"nodes":[], "connections":[]})
}
fn checked_name(value: &str) -> Result<String, String> {
    let name = value.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        Err("请输入 1 至 80 个字符的画布名称".into())
    } else {
        Ok(name.to_owned())
    }
}
fn validate_graph(graph: &Value) -> Result<(), String> {
    if !graph["nodes"].as_array().is_some_and(|n| n.len() <= 500)
        || !graph["connections"]
            .as_array()
            .is_some_and(|c| c.len() <= 2000)
        || serde_json::to_vec(graph).map_err(|_| "画布数据无效")?.len() > MAX_GRAPH_BYTES
    {
        return Err("画布数据无效或过大，未覆盖原内容".into());
    }
    Ok(())
}
fn read_json(path: &Path, max: usize) -> Result<Value, String> {
    let length = std::fs::metadata(path)
        .map_err(|_| "无法读取画布文件，已保留原列表")?
        .len();
    if length > max as u64 {
        return Err("画布文件过大，已保留原文件".into());
    }
    let bytes = std::fs::read(path).map_err(|_| "无法读取画布文件")?;
    serde_json::from_slice(&bytes).map_err(|_| "画布文件损坏，已保留原文件".into())
}
fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|_| "无法保存画布")?;
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, bytes).map_err(|_| "画布保存失败")?;
    std::fs::rename(temp, path).map_err(|_| "画布保存失败".into())
}
