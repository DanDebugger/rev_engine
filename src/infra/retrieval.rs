use pgvector::Vector;
use sqlx::PgPool;

pub async fn search_similar_chunks(pool: &PgPool, query_embedding: Vec<f32>, limit: i64) -> Result<Vec<String>, String> {
    let query_vector = Vector::from(query_embedding);
    
    // Perform vector similarity search using <=> (cosine distance)
    let rows = sqlx::query_as::<_, (String,)>(
        r#"
        SELECT content 
        FROM document_chunks
        ORDER BY embedding <=> $1
        LIMIT $2
        "#
    )
    .bind(query_vector)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Database query failed: {}", e))?;

    Ok(rows.into_iter().map(|(c,)| c).collect())
}
