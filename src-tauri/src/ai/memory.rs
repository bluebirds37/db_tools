use crate::models::*;
use chrono::Utc;
use uuid::Uuid;

pub fn add_fact(store: &mut MemoryStore, content: &str) {
    store.facts.push(MemoryFact {
        id: Uuid::new_v4().to_string(),
        content: content.to_string(),
        created_at: Utc::now().to_rfc3339(),
    });
    // keep last 100
    if store.facts.len() > 100 {
        let n = store.facts.len() - 100;
        store.facts.drain(0..n);
    }
}

pub fn list_fact_strings(store: &MemoryStore) -> Vec<String> {
    store.facts.iter().map(|f| f.content.clone()).collect()
}
