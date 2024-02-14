/// # How this example was set up
///
/// Start a temporary postgres server in single mode:
/// `pgtemp --single postgresql://postgres@localhost:5432`
///
/// Install the diesel CLI
/// `cargo install diesel_cli --no-default-features --features postgres`
///
/// Run diesel setup and diesel migration
/// ```
/// diesel setup --database-url "postgresql://postgres@localhost:5432" --migrations-dir "examples/diesel-migrations"
/// diesel migration --database-url "postgresql://postgres@localhost:5432" --migrations-dir "examples/diesel-migrations" \
///   generate create_tasks_table
/// ```
///
/// A diesel.toml and src/schema.rs are generated by the previous commands. The contents of
/// src/schema.rs is moved into the current file and the diesel.toml is deleted to make this
/// example self-contained.
///
/// The pgtemp cli doesn't need to be running during the execution of the example, only for setup
/// with diesel.
use axum::{
    extract::State,
    routing::{get, post},
    Router,
};

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::r2d2::ConnectionManager;
use diesel::r2d2::Pool;

use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("examples/diesel-migrations");

type PgPool = Pool<ConnectionManager<PgConnection>>;

diesel::table! {
    tasks (id) {
        id -> Int4,
        task -> Text,
    }
}

#[derive(Selectable, Queryable)]
struct Task {
    task: String,
}

#[derive(Insertable)]
#[diesel(table_name = tasks)]
struct NewTask {
    task: String,
}

fn connection_pool(conn_uri: &str) -> PgPool {
    let manager = ConnectionManager::<PgConnection>::new(conn_uri);
    let pool = Pool::builder()
        .build(manager)
        .expect("failed to build connection pool");
    let mut conn = pool.get().expect("failed to get connection from pool");

    // run migrations on pool creation
    conn.run_pending_migrations(MIGRATIONS)
        .expect("failed to run migrations");

    pool
}

async fn list_tasks(pool: State<PgPool>) -> String {
    // NOTE: diesel and r2d2 are sync so we should really use spawn_blocking here
    let mut conn = pool.get().expect("failed to get connection from pool");

    let tasks: Vec<Task> = tasks::table
        .select(Task::as_select())
        .load(&mut conn)
        .expect("failed to load tasks");

    tasks
        .into_iter()
        .fold(String::new(), |s, t| s + "\n" + &t.task)
}

async fn create_task(pool: State<PgPool>, body: String) -> &'static str {
    // NOTE: diesel and r2d2 are sync so we should really use spawn_blocking here
    let mut conn = pool.get().expect("failed to get connection from pool");

    let new_task = NewTask { task: body };

    diesel::insert_into(tasks::table)
        .values(new_task)
        .execute(&mut conn)
        .expect("failed to insert task");

    "ok"
}

fn axum_router(pool: PgPool) -> Router {
    Router::new()
        .route("/list_tasks", get(list_tasks))
        .route("/create_task", post(create_task))
        .with_state(pool)
}

#[tokio::test]
async fn test_diesel_example() {
    run_diesel_example().await;
}

#[tokio::main]
async fn main() {
    run_diesel_example().await;
}

async fn run_diesel_example() {
    // start db
    let db = pgtemp::PgTempDB::new();
    let conn_uri = db.connection_uri().clone();

    let pool = connection_pool(&conn_uri);

    // create axum router and spawn listener
    let router = axum_router(pool);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to start listener");
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("failed to run axum server");
    });

    // add two tasks
    let base_url = format!("http://{addr}");
    let client = reqwest::Client::new();

    let resp = client
        .post(base_url.clone() + "/create_task")
        .body("hello")
        .send()
        .await
        .expect("failed to create task 1");
    assert!(resp.status().is_success());

    let resp = client
        .post(base_url.clone() + "/create_task")
        .body("task 2")
        .send()
        .await
        .expect("failed to create task 2");
    assert!(resp.status().is_success());

    // query tasks
    let resp = client
        .get(base_url + "/list_tasks")
        .send()
        .await
        .expect("failed to list tasks");
    assert!(resp.status().is_success());

    let body = resp.text().await.expect("failed to parse body");
    assert!(body.contains("hello"));
    assert!(body.contains("task 2"));
}