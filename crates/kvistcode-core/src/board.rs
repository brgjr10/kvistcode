//! Project task board: the Backlog column. Cards are notes about work not started yet; the other
//! columns (Working, Needs review, Merged) are derived from real agent state in the UI.

use anyhow::{bail, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::model::now_ms;

const MAX_TITLE: usize = 200;
const MAX_NOTES: usize = 20_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BoardCard {
    pub id: String,
    pub project_path: String,
    pub title: String,
    pub notes: String,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

fn clean(title: &str, notes: &str) -> Result<(String, String)> {
    let title = title.trim();
    if title.is_empty() {
        bail!("A card needs a title");
    }
    if title.chars().count() > MAX_TITLE || notes.len() > MAX_NOTES {
        bail!("That card is too long");
    }
    Ok((title.to_string(), notes.trim().to_string()))
}

impl Db {
    /// A project's backlog cards, in the order the user keeps them.
    pub fn board_cards(&self, project_path: &str) -> Result<Vec<BoardCard>> {
        let conn = self.write_conn();
        let mut stmt = conn.prepare(
            "SELECT id, project_path, title, notes, position, created_at, updated_at
             FROM board_cards WHERE project_path = ?1 ORDER BY position, created_at",
        )?;
        let rows = stmt.query_map(params![project_path], |r| {
            Ok(BoardCard {
                id: r.get(0)?,
                project_path: r.get(1)?,
                title: r.get(2)?,
                notes: r.get(3)?,
                position: r.get(4)?,
                created_at: r.get(5)?,
                updated_at: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Adds a card at the end of the project's backlog.
    pub fn add_board_card(
        &self,
        project_path: &str,
        title: &str,
        notes: &str,
    ) -> Result<BoardCard> {
        let (title, notes) = clean(title, notes)?;
        let conn = self.write_conn();
        let position: i64 = conn.query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM board_cards WHERE project_path = ?1",
            params![project_path],
            |r| r.get(0),
        )?;
        let now = now_ms();
        let card = BoardCard {
            id: uuid::Uuid::new_v4().to_string(),
            project_path: project_path.to_string(),
            title,
            notes,
            position,
            created_at: now,
            updated_at: now,
        };
        conn.execute(
            "INSERT INTO board_cards (id, project_path, title, notes, position, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![card.id, card.project_path, card.title, card.notes, card.position, now],
        )?;
        Ok(card)
    }

    pub fn update_board_card(&self, id: &str, title: &str, notes: &str) -> Result<()> {
        let (title, notes) = clean(title, notes)?;
        let n = self.write_conn().execute(
            "UPDATE board_cards SET title = ?2, notes = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, title, notes, now_ms()],
        )?;
        if n == 0 {
            bail!("That card is gone");
        }
        Ok(())
    }

    /// Deleting a card that is already gone is fine (started from another window).
    pub fn delete_board_card(&self, id: &str) -> Result<()> {
        self.write_conn()
            .execute("DELETE FROM board_cards WHERE id = ?1", params![id])?;
        Ok(())
    }
}

impl crate::manager::Core {
    pub fn board_cards(&self, project_path: &str) -> Result<Vec<BoardCard>> {
        self.db().board_cards(project_path)
    }

    pub fn add_board_card(
        &self,
        project_path: &str,
        title: &str,
        notes: &str,
    ) -> Result<BoardCard> {
        self.db().add_board_card(project_path, title, notes)
    }

    pub fn update_board_card(&self, id: &str, title: &str, notes: &str) -> Result<()> {
        self.db().update_board_card(id, title, notes)
    }

    pub fn delete_board_card(&self, id: &str) -> Result<()> {
        self.db().delete_board_card(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db(name: &str) -> (Db, std::path::PathBuf) {
        let dir = crate::testutil::scratch(&format!("board-{name}"));
        (Db::open(&dir.join("armada.db")).unwrap(), dir)
    }

    #[test]
    fn cards_keep_order_per_project() {
        let (db, _dir) = db("order");
        let a = db.add_board_card("/p/a", "  First ", "notes").unwrap();
        db.add_board_card("/p/a", "Second", "").unwrap();
        db.add_board_card("/p/b", "Other", "").unwrap();
        let cards = db.board_cards("/p/a").unwrap();
        let titles: Vec<_> = cards.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["First", "Second"]);
        assert_eq!(cards[0].id, a.id);
        assert_eq!(db.board_cards("/p/b").unwrap().len(), 1);
    }

    #[test]
    fn edit_and_delete() {
        let (db, _dir) = db("edit");
        let c = db.add_board_card("/p", "Old", "").unwrap();
        db.update_board_card(&c.id, "New", "why").unwrap();
        let cards = db.board_cards("/p").unwrap();
        assert_eq!(
            (cards[0].title.as_str(), cards[0].notes.as_str()),
            ("New", "why")
        );
        assert!(db.update_board_card("nope", "x", "").is_err());
        db.delete_board_card(&c.id).unwrap();
        db.delete_board_card(&c.id).unwrap();
        assert!(db.board_cards("/p").unwrap().is_empty());
    }

    #[test]
    fn rejects_empty_and_huge_titles() {
        let (db, _dir) = db("bad");
        assert!(db.add_board_card("/p", "   ", "").is_err());
        assert!(db
            .add_board_card("/p", &"x".repeat(MAX_TITLE + 1), "")
            .is_err());
    }

    #[test]
    fn cards_survive_reopen() {
        let (db, dir) = db("reopen");
        db.add_board_card("/p", "Keep", "").unwrap();
        drop(db);
        let db = Db::open(&dir.join("armada.db")).unwrap();
        assert_eq!(db.board_cards("/p").unwrap().len(), 1);
    }
}
