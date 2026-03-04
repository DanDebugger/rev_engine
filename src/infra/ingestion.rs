use crate::infra::embedding::generate_embedding;
use pgvector::Vector;
use sqlx::PgPool;
use text_splitter::TextSplitter;
use scraper::{Html, Selector};
use pdf_extract::extract_text_from_mem;
use uuid::Uuid;

pub enum DocumentSource {
    Text(String),
    Url(String),
    Pdf(Vec<u8>),
}

pub async fn ingest_document(
    pool: &PgPool,
    title: &str,
    source: DocumentSource,
    source_url: Option<String>,
) -> Result<Uuid, String> {
    // 1. Extract raw text
    let (raw_text, source_type) = match source {
        DocumentSource::Text(text) => (text, "text"),
        DocumentSource::Url(url) => {
            let body = reqwest::get(&url)
                .await
                .map_err(|e| format!("Failed to fetch URL: {}", e))?
                .text()
                .await
                .map_err(|e| format!("Failed to read response: {}", e))?;
            
            let document = Html::parse_document(&body);
            let body_selector = Selector::parse("body").unwrap();
            let mut extracted = String::new();
            if let Some(body_elem) = document.select(&body_selector).next() {
                for text in body_elem.text() {
                    let cleaned = text.trim();
                    if !cleaned.is_empty() {
                        extracted.push_str(cleaned);
                        extracted.push(' ');
                    }
                }
            } else {
                return Err("Failed to parse HTML body".to_string());
            }
            (extracted, "web")
        },
        DocumentSource::Pdf(bytes) => {
            let text = extract_text_from_mem(&bytes)
                .map_err(|e| format!("Failed to extract PDF: {:?}", e))?;
            (text, "pdf")
        }
    };

    if raw_text.trim().is_empty() {
        return Err("Extracted text is empty".to_string());
    }

    // 2. Insert Document metadata
    let doc_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO documents (title, source_type, source_url) VALUES ($1, $2, $3) RETURNING id"
    )
    .bind(title)
    .bind(source_type)
    .bind(source_url)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to insert document: {}", e))?;

    // 3. Chunk text
    // Using roughly 1000 character chunks max
    let splitter = TextSplitter::new(1000);
    let chunks: Vec<&str> = splitter.chunks(&raw_text).collect();

    if chunks.is_empty() {
        return Ok(doc_id);
    }

    // 4. Generate Embeddings & Insert chunks
    for (i, chunk) in chunks.iter().enumerate() {
        let embedding = generate_embedding(chunk).await?;
        let vector = Vector::from(embedding);

        sqlx::query(
            "INSERT INTO document_chunks (document_id, chunk_index, content, embedding) VALUES ($1, $2, $3, $4)"
        )
        .bind(doc_id)
        .bind(i as i32)
        .bind(chunk)
        .bind(vector)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to insert chunk {}: {}", i, e))?;
    }

    Ok(doc_id)
}
