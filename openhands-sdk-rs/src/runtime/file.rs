use crate::models::{FileReadRequest, FileResponse, FileWriteRequest};
use std::fs;
use std::path::PathBuf;

pub struct FileService {
    pub workspace_dir: PathBuf,
}

impl FileService {
    pub fn new(workspace_dir: PathBuf) -> Self {
        fs::create_dir_all(&workspace_dir).expect("Failed to create workspace dir");
        Self { workspace_dir }
    }

    pub fn read_file(&self, req: FileReadRequest) -> FileResponse {
        let path = self.workspace_dir.join(&req.path);
        match fs::read_to_string(&path) {
            Ok(content) => FileResponse {
                path: req.path,
                content: Some(content),
                success: true,
                error: None,
            },
            Err(e) => FileResponse {
                path: req.path,
                content: None,
                success: false,
                error: Some(e.to_string()),
            },
        }
    }

    pub fn write_file(&self, req: FileWriteRequest) -> FileResponse {
        let path = self.workspace_dir.join(&req.path);

        if let Some(parent) = path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                return FileResponse {
                    path: req.path,
                    content: None,
                    success: false,
                    error: Some(format!("Failed to create parent directory: {}", e)),
                };
            }
        }

        match fs::write(&path, &req.content) {
            Ok(_) => FileResponse {
                path: req.path,
                content: None,
                success: true,
                error: None,
            },
            Err(e) => FileResponse {
                path: req.path,
                content: None,
                success: false,
                error: Some(e.to_string()),
            },
        }
    }
}
