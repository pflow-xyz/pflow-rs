use std::str;

use rusqlite::{Connection, params, Result};
use serde::{Serialize, Serializer};

use pflow_metamodel::oid::Oid;
use pflow_metamodel::compression::encode_zip;
use pflow_metamodel::petri_net::PetriNet;

#[derive(Debug, Clone, Serialize)]
pub struct Zblob {
    pub id: i64,
    pub ipfs_cid: String,
    pub base64_zipped: String,
    pub title: String,
    pub description: String,
    pub keywords: String,
    pub referrer: String,
    pub created_at: String,
}

impl Default for Zblob {
    fn default() -> Self {
        let net = serde_json::to_string(&PetriNet::new()).unwrap();
        let data = encode_zip(&net, "model.json");
        Self {
            id: 0,
            ipfs_cid: Oid::new(data.as_bytes()).unwrap().to_string(),
            base64_zipped: data,
            title: "".to_string(),
            description: "".to_string(),
            keywords: "".to_string(),
            referrer: "".to_string(),
            created_at: "".to_string(),
        }
    }
}

pub struct Storage {
    conn: Connection,
}


impl Storage {
    pub fn new(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        Ok(Self { conn: conn })
    }

    pub fn create_tables(&self) -> Result<()> {
        let tables = vec!["pflow_models"];
        for table in tables {
            self.create_blob_table(table)?;
        }
        Ok(())
    }

    pub fn create_blob_table(&self, table_name: &str) -> Result<()> {
        let create_sql = format!(
            "CREATE TABLE IF NOT EXISTS {} (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ipfs_cid TEXT UNIQUE,
                base64_zipped TEXT,
                title TEXT,
                description TEXT,
                keywords TEXT,
                referrer TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );",
            table_name
        );
        self.conn.execute(&create_sql, ())?;
        Ok(())
    }

    pub fn reset_db(&self, drop_tables: bool) -> Result<()> {
        if drop_tables {
            let tables = vec!["pflow_models"];
            for table in tables {
                let drop_sql = format!("DROP TABLE IF EXISTS {}", table);
                self.conn.execute(&drop_sql, ())?;
            }
        }
        self.create_tables()?;
        Ok(())
    }

    pub fn create_or_retrieve(
        &self,
        table_name: &str,
        ipfs_cid: &str,
        base64_zipped: &str,
        title: &str,
        description: &str,
        keywords: &str,
        referrer: &str,
    ) -> Result<Zblob> {
        // Check if a record with the same ipfs_cid already exists
        if let Some(existing_record) = self.get_by_cid(table_name, ipfs_cid)? {
            return Ok(existing_record);
        }

        // Attempt to insert a new record
        let insert_sql = format!(
            "INSERT INTO {} (ipfs_cid, base64_zipped, title, description, keywords, referrer) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            table_name
        );
        let mut stmt = self.conn.prepare(&insert_sql)?;
        match stmt.execute(params![
            ipfs_cid,
            base64_zipped,
            title,
            description,
            keywords,
            referrer
        ]) {
            Ok(_) => {
                // If the insert is successful, return the newly created record
                println!("Inserted new zblob with ipfs_cid: {}", ipfs_cid);
                self.get_by_cid(table_name, ipfs_cid)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
            }
            Err(e) => {
                panic!("Failed to insert new zblob: {}", e);
            }
        }
    }

    pub fn get_id_from_cid(&self, table_name: &str, ipfs_cid: &str) -> Result<i32> {
        let select_sql = format!("SELECT id FROM {} WHERE ipfs_cid = ?1", table_name);
        let mut stmt = self.conn.prepare(&select_sql)?;
        let mut rows = stmt.query(params![ipfs_cid])?;
        if let Some(row) = rows.next()? {
            Ok(row.get(0)?)
        } else {
            Ok(0)
        }
    }

    pub fn get_by_cid(&self, table_name: &str, ipfs_cid: &str) -> Result<Option<Zblob>> {
        let select_sql = format!("SELECT * FROM {} WHERE ipfs_cid = ?1", table_name);
        let mut stmt = self.conn.prepare(&select_sql)?;
        let mut rows = stmt.query(params![ipfs_cid])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Zblob {
                id: row.get(0)?,
                ipfs_cid: row.get(1)?,
                base64_zipped: row.get(2)?,
                title: row.get(3)?,
                description: row.get(4)?,
                keywords: row.get(5)?,
                referrer: row.get(6)?,
                created_at: row.get(7)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn get_max_id(&self, table_name: &str) -> Result<i64> {
        let select_sql = format!("SELECT MAX(id) FROM {}", table_name);
        let mut stmt = self.conn.prepare(&select_sql)?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            Ok(row.get(0)?)
        } else {
            Ok(0)
        }
    }
}