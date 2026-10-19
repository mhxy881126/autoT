use std::path::Path;

use rusqlite::Connection;

/// 打开账本并执行 schema。路径由调用方决定（app_data_dir/desk.db）。
pub fn open(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| format!("打开数据库失败: {e}"))?;
    conn.execute_batch(include_str!("../../../contracts/schema.sql"))
        .map_err(|e| format!("初始化数据库失败: {e}"))?;
    Ok(conn)
}
