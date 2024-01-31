use tracing::info;

pub mod cookie;

pub fn ensure_file(filename: &String) -> anyhow::Result<()> {
    let file_path = std::path::Path::new(filename);

    if !file_path.exists() {
        if let Some(parent) = file_path.parent() {
            info!("file doesn't exist, create dir all: {}", filename);
            std::fs::create_dir_all(parent)?;
        }
        info!("file doesn't exist, create it: {}", filename);
        std::fs::File::create(file_path)?;
    }

    Ok(())
}
