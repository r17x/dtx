use anyhow::Result;

use dtx_core::config::project::find_project_root_cwd;
use dtx_memory::{Memory, MemoryKind, MemoryMeta, MemoryStore};

use crate::output::Output;

fn store() -> Result<MemoryStore> {
    let root = find_project_root_cwd()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()));
    MemoryStore::new(root.join(".dtx").join("memories")).map_err(|e| anyhow::anyhow!("{e}"))
}

pub async fn list(out: &Output) -> Result<()> {
    let s = store()?;
    let metas = s.list().map_err(|e| anyhow::anyhow!("{e}"))?;

    out.step("memories")
        .done(&format!("{} memories", metas.len()));
    for m in &metas {
        let desc = m.description.as_deref().unwrap_or("");
        println!("  {} ({}) — {desc}", m.name, m.kind);
    }
    Ok(())
}

pub async fn read(out: &Output, name: String) -> Result<()> {
    let s = store()?;
    let memory = s.read(&name).map_err(|e| anyhow::anyhow!("{e}"))?;

    out.step("memory").done(&name);
    println!("{}", memory.to_file_content());
    Ok(())
}

pub async fn write(
    out: &Output,
    name: String,
    content: String,
    kind: Option<String>,
    description: Option<String>,
) -> Result<()> {
    let s = store()?;
    let kind: MemoryKind = kind
        .as_deref()
        .unwrap_or("project")
        .parse()
        .map_err(|e: String| anyhow::anyhow!(e))?;

    let now = chrono::Utc::now();
    let memory = Memory {
        meta: MemoryMeta {
            name: name.clone(),
            kind,
            description,
            created_at: now,
            updated_at: now,
            tags: vec![],
        },
        content,
    };
    s.write(&memory).map_err(|e| anyhow::anyhow!("{e}"))?;
    out.step("write").done(&format!("memory '{name}' written"));
    Ok(())
}

pub async fn delete(out: &Output, name: String) -> Result<()> {
    let s = store()?;
    s.delete(&name).map_err(|e| anyhow::anyhow!("{e}"))?;
    out.step("delete").done(&format!("memory '{name}' deleted"));
    Ok(())
}
