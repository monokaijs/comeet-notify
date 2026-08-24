use std::env;

#[derive(Clone, Debug)]
pub struct FirebaseConfig {
    pub project_id: String,
    pub private_key: String,
    pub client_email: String,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub port: u16,
    pub environment: String,
    pub log_level: String,
    pub firebase: Option<FirebaseConfig>,
}

impl Config {
    pub fn load() -> Self {
        // dotenv does not overwrite process variables. Loading local first preserves
        // the precedence used by the previous NestJS configuration.
        let _ = dotenvy::from_filename(".env.local");
        let _ = dotenvy::from_filename(".env");

        let project_id = non_empty("FIREBASE_PROJECT_ID");
        let private_key = non_empty("FIREBASE_PRIVATE_KEY").map(|key| key.replace("\\n", "\n"));
        let client_email = non_empty("FIREBASE_CLIENT_EMAIL");
        let firebase = match (project_id, private_key, client_email) {
            (Some(project_id), Some(private_key), Some(client_email)) => Some(FirebaseConfig {
                project_id,
                private_key,
                client_email,
            }),
            _ => None,
        };

        Self {
            port: env::var("PORT")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(3000),
            environment: env::var("NODE_ENV").unwrap_or_else(|_| "development".to_owned()),
            log_level: env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_owned()),
            firebase,
        }
    }
}

fn non_empty(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}
