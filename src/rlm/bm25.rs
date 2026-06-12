use std::collections::{HashMap, HashSet};

const K1: f64 = 1.2;
const B: f64 = 0.75;

pub struct Bm25Scorer {
    doc_freq: HashMap<String, usize>,
    avgdl: f64,
    n_docs: usize,
}

impl Bm25Scorer {
    pub fn from_documents(docs: &[Vec<String>]) -> Self {
        let n_docs = docs.len().max(1);
        let mut doc_freq = HashMap::new();
        let mut total_len = 0usize;

        for doc in docs {
            total_len += doc.len();
            let mut seen = HashSet::new();
            for tok in doc {
                if seen.insert(tok.clone()) {
                    *doc_freq.entry(tok.clone()).or_default() += 1;
                }
            }
        }

        Self {
            doc_freq,
            avgdl: total_len as f64 / n_docs as f64,
            n_docs,
        }
    }

    pub fn score(&self, query: &[String], doc: &[String]) -> f64 {
        if doc.is_empty() || query.is_empty() {
            return 0.0;
        }

        let mut term_freq = HashMap::new();
        for t in doc {
            *term_freq.entry(t.as_str()).or_default() += 1;
        }

        let dl = doc.len() as f64;
        let avgdl = self.avgdl.max(1.0);
        let mut total = 0.0;

        for q in query {
            let f = *term_freq.get(q.as_str()).unwrap_or(&0) as f64;
            if f == 0.0 {
                continue;
            }
            let df = *self.doc_freq.get(q).unwrap_or(&0) as f64;
            let idf = ((self.n_docs as f64 - df + 0.5) / (df + 0.5) + 1.0).ln();
            let numerator = f * (K1 + 1.0);
            let denominator = f + K1 * (1.0 - B + B * dl / avgdl);
            total += idf * numerator / denominator;
        }

        total
    }
}

pub fn tokenize(text: &str, case_sensitive: bool) -> Vec<String> {
    let source = if case_sensitive {
        text.to_string()
    } else {
        text.to_lowercase()
    };
    source
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 1)
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_matching_document_higher() {
        let docs = vec![
            tokenize("disk full on volume data partition", false),
            tokenize("info heartbeat ok", false),
        ];
        let scorer = Bm25Scorer::from_documents(&docs);
        let query = tokenize("disk volume data", false);
        let s0 = scorer.score(&query, &docs[0]);
        let s1 = scorer.score(&query, &docs[1]);
        assert!(s0 > s1);
        assert!(s0 > 0.0);
    }

    #[test]
    fn tokenize_splits_needle_key() {
        let tokens = tokenize("NEEDLE_KEY=MAGIC-42", false);
        assert!(tokens.contains(&"needle".to_string()));
        assert!(tokens.contains(&"key".to_string()));
        assert!(tokens.contains(&"magic".to_string()));
    }
}
