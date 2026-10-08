//! A sqllogictest engine backed by a real PostGIS instance, used as the oracle.

use sqllogictest::{AsyncDB, DBOutput, DefaultColumnType};
use tokio_postgres::types::Type;
use tokio_postgres::{Client, NoTls, SimpleQueryMessage};

use crate::{EngineError, render};

pub const DEFAULT_URL: &str = "postgresql://postgres:postgres@localhost:54329/postgres";

pub fn url() -> String {
    std::env::var("POSTGIS_URL").unwrap_or_else(|_| DEFAULT_URL.to_string())
}

/// PostGIS's aggregates under the names geodatafusion gives them, where a scalar function has the
/// PostGIS name (plans D8): `st_collect_agg` is PostGIS's aggregate `ST_Collect(geometry)`.
///
/// Each collects its input into an array and finishes with PostGIS's own aggregate over it, in
/// input order, so PostGIS stays the oracle. (PostGIS's `geometry[]` overloads aren't always the
/// same: `ST_Union` of `POINT EMPTY` and `POINT Z (1 2 3)` drops the Z.) Like everything else in
/// the file's transaction, they are rolled back at the end.
const AGGREGATE_ALIASES: &str = "
    CREATE FUNCTION pg_temp.collect_agg_final(geoms geometry[])
        RETURNS geometry LANGUAGE sql IMMUTABLE
        AS $$ SELECT ST_Collect(geom ORDER BY i) FROM unnest(geoms) WITH ORDINALITY AS t(geom, i) $$;
    CREATE AGGREGATE st_collect_agg(geometry)
        (SFUNC = array_append, STYPE = geometry[], FINALFUNC = pg_temp.collect_agg_final);
    CREATE FUNCTION pg_temp.makeline_agg_final(geoms geometry[])
        RETURNS geometry LANGUAGE sql IMMUTABLE
        AS $$ SELECT ST_MakeLine(geom ORDER BY i) FROM unnest(geoms) WITH ORDINALITY AS t(geom, i) $$;
    CREATE AGGREGATE st_makeline_agg(geometry)
        (SFUNC = array_append, STYPE = geometry[], FINALFUNC = pg_temp.makeline_agg_final);
    CREATE FUNCTION pg_temp.union_agg_final(geoms geometry[])
        RETURNS geometry LANGUAGE sql IMMUTABLE
        AS $$ SELECT ST_Union(geom ORDER BY i) FROM unnest(geoms) WITH ORDINALITY AS t(geom, i) $$;
    CREATE AGGREGATE st_union_agg(geometry)
        (SFUNC = array_append, STYPE = geometry[], FINALFUNC = pg_temp.union_agg_final);
    CREATE TYPE pg_temp.union_agg_state AS (geoms geometry[], gridsize float8);
    CREATE FUNCTION pg_temp.union_agg_step(
        state pg_temp.union_agg_state, geom geometry, gridsize float8
    ) RETURNS pg_temp.union_agg_state LANGUAGE sql IMMUTABLE
        AS $$ SELECT ROW(array_append(state.geoms, geom), gridsize)::pg_temp.union_agg_state $$;
    CREATE FUNCTION pg_temp.union_agg_final(state pg_temp.union_agg_state)
        RETURNS geometry LANGUAGE sql IMMUTABLE
        AS $$
            SELECT ST_Union(geom, state.gridsize ORDER BY i)
            FROM unnest(state.geoms) WITH ORDINALITY AS t(geom, i)
        $$;
    CREATE AGGREGATE st_union_agg(geometry, float8) (
        SFUNC = pg_temp.union_agg_step, STYPE = pg_temp.union_agg_state,
        FINALFUNC = pg_temp.union_agg_final, INITCOND = '({},)'
    );
";

pub struct PostGIS {
    client: Client,
}

impl PostGIS {
    /// Connect and open a transaction that is rolled back at the end of the file, so that tables
    /// created by one `.slt` file never leak into another.
    pub async fn connect() -> Result<Self, EngineError> {
        let url = url();
        let (client, connection) = tokio_postgres::connect(&url, NoTls).await.map_err(|e| {
            EngineError(format!(
                "could not connect to PostGIS at {url}: {e}\n\
                 Start it with `dev/postgis.sh start` or set POSTGIS_URL."
            ))
        })?;
        tokio::spawn(connection);
        client
            .batch_execute(
                "SET client_min_messages = warning;
                 SET statement_timeout = '30s';
                 SET TimeZone = 'UTC';
                 BEGIN;",
            )
            .await?;
        client.batch_execute(AGGREGATE_ALIASES).await?;
        Ok(Self { client })
    }

    async fn run_inner(&mut self, sql: &str) -> Result<DBOutput<DefaultColumnType>, EngineError> {
        // Prepare first to learn the column types; the simple query protocol only gives us text.
        let statement = self.client.prepare(sql).await?;
        let types: Vec<Type> = statement
            .columns()
            .iter()
            .map(|c| c.type_().clone())
            .collect();

        let messages = self.client.simple_query(sql).await?;
        if types.is_empty() {
            let count = messages
                .iter()
                .find_map(|m| match m {
                    SimpleQueryMessage::CommandComplete(n) => Some(*n),
                    _ => None,
                })
                .unwrap_or(0);
            return Ok(DBOutput::StatementComplete(count));
        }

        let mut rows = vec![];
        for message in messages {
            if let SimpleQueryMessage::Row(row) = message {
                let rendered = types
                    .iter()
                    .enumerate()
                    .map(|(i, ty)| match row.get(i) {
                        None => render::NULL.to_string(),
                        Some(v) => render_value(ty, v),
                    })
                    .collect();
                rows.push(rendered);
            }
        }
        Ok(DBOutput::Rows {
            types: types.iter().map(column_type).collect(),
            rows,
        })
    }
}

fn column_type(ty: &Type) -> DefaultColumnType {
    match ty.name() {
        "int2" | "int4" | "int8" => DefaultColumnType::Integer,
        "float4" | "float8" | "numeric" => DefaultColumnType::FloatingPoint,
        _ => DefaultColumnType::Text,
    }
}

fn render_scalar(type_name: &str, v: &str) -> String {
    match type_name {
        "geometry" | "geography" => match render::decode_hex(v) {
            Some(buf) => render::ewkb(&buf, None),
            None => render::text(v),
        },
        "float4" | "float8" | "numeric" => v
            .parse::<f64>()
            .map(render::float)
            .unwrap_or_else(|_| render::text(v)),
        "bool" => match v {
            "t" => "true".to_string(),
            "f" => "false".to_string(),
            other => other.to_string(),
        },
        "box2d" | "box3d" => render::pg_box(v),
        // Already hex-encoded as `\x...`, the same form render::bytes produces.
        "bytea" => v.to_string(),
        _ => render::text(v),
    }
}

fn render_value(ty: &Type, v: &str) -> String {
    if let tokio_postgres::types::Kind::Array(element) = ty.kind() {
        if let Some(elements) = parse_pg_array(v) {
            let rendered: Vec<String> = elements
                .iter()
                .map(|e| match e {
                    None => render::NULL.to_string(),
                    Some(e) => render_scalar(element.name(), e),
                })
                .collect();
            return format!("{{{}}}", rendered.join(","));
        }
        return render::text(v);
    }
    render_scalar(ty.name(), v)
}

/// Parse a one-dimensional Postgres array literal like `{a,"b c",NULL}`.
fn parse_pg_array(s: &str) -> Option<Vec<Option<String>>> {
    let inner = s.strip_prefix('{')?.strip_suffix('}')?;
    if inner.is_empty() {
        return Some(vec![]);
    }
    if inner.starts_with('{') {
        // Multi-dimensional arrays are left as text.
        return None;
    }
    let mut out = vec![];
    let mut chars = inner.chars().peekable();
    loop {
        let mut value = String::new();
        let quoted = chars.peek() == Some(&'"');
        if quoted {
            chars.next();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => value.extend(chars.next()),
                    '"' => break,
                    c => value.push(c),
                }
            }
        }
        while let Some(&c) = chars.peek() {
            if c == ',' {
                break;
            }
            value.push(c);
            chars.next();
        }
        out.push(if !quoted && value == "NULL" {
            None
        } else {
            Some(value)
        });
        if chars.next().is_none() {
            break;
        }
    }
    Some(out)
}

#[async_trait::async_trait]
impl AsyncDB for PostGIS {
    type Error = EngineError;
    type ColumnType = DefaultColumnType;

    async fn run(&mut self, sql: &str) -> Result<DBOutput<Self::ColumnType>, Self::Error> {
        // Each record runs in a savepoint so that an error doesn't abort the file's transaction.
        self.client.batch_execute("SAVEPOINT slt_record").await?;
        let result = self.run_inner(sql).await;
        let cleanup = if result.is_ok() {
            "RELEASE SAVEPOINT slt_record"
        } else {
            "ROLLBACK TO SAVEPOINT slt_record"
        };
        self.client.batch_execute(cleanup).await?;
        result
    }

    async fn shutdown(&mut self) {
        let _ = self.client.batch_execute("ROLLBACK").await;
    }

    fn engine_name(&self) -> &str {
        "postgis"
    }

    async fn sleep(dur: std::time::Duration) {
        tokio::time::sleep(dur).await
    }
}
