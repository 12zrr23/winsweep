use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LargeFileInfo {
    pub file_name: String,
    pub path: PathBuf,
    pub size: u64,
}

#[derive(Debug, Eq, PartialEq)]
struct FileCandidate {
    size: u64,
    path: PathBuf,
    name: String,
}

impl Ord for FileCandidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.size.cmp(&other.size)
    }
}

impl PartialOrd for FileCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DiskAnalysisReport {
    pub target_path: String,
    pub exists: bool,
    pub total_scanned_files: usize,
    pub total_scanned_bytes: u64,
    pub largest_files: Vec<LargeFileInfo>,
}

pub struct DiskAnalyzer;

impl DiskAnalyzer {
    pub fn new() -> Self {
        Self
    }

    /// 深度分析指定目录并按体积准确定位 Top N 大文件（采用高效小顶堆算法，彻底解除 50MB 硬编码限制）
    pub fn analyze_path(&self, root: &Path, max_large_files: usize) -> DiskAnalysisReport {
        let target_str = root.to_string_lossy().to_string();

        if !root.exists() {
            return DiskAnalysisReport {
                target_path: target_str,
                exists: false,
                total_scanned_files: 0,
                total_scanned_bytes: 0,
                largest_files: Vec::new(),
            };
        }

        let mut total_files = 0usize;
        let mut total_bytes = 0u64;
        let mut heap: BinaryHeap<Reverse<FileCandidate>> = BinaryHeap::new();
        let limit = max_large_files.max(1);

        let walker = WalkDir::new(root)
            .min_depth(1)
            .max_depth(8)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                name != "$RECYCLE.BIN" && name != "System Volume Information"
            });

        for entry in walker.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file() {
                total_files += 1;
                if let Ok(meta) = fs::metadata(path) {
                    let size = meta.len();
                    total_bytes += size;

                    let file_name = path
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Unknown".to_string());

                    let candidate = FileCandidate {
                        size,
                        path: path.to_path_buf(),
                        name: file_name,
                    };

                    if heap.len() < limit {
                        heap.push(Reverse(candidate));
                    } else if let Some(min_top) = heap.peek() {
                        if size > min_top.0.size {
                            heap.pop();
                            heap.push(Reverse(candidate));
                        }
                    }
                }
            }
        }

        // 转为按体积从大到小排序的列表
        let mut largest_files: Vec<LargeFileInfo> = heap
            .into_sorted_vec()
            .into_iter()
            .map(|Reverse(c)| LargeFileInfo {
                file_name: c.name,
                path: c.path,
                size: c.size,
            })
            .collect();

        largest_files.sort_by(|a, b| b.size.cmp(&a.size));

        DiskAnalysisReport {
            target_path: target_str,
            exists: true,
            total_scanned_files: total_files,
            total_scanned_bytes: total_bytes,
            largest_files,
        }
    }
}
