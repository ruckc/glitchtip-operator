use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("kubernetes api error: {0}")]
    Kube(#[from] kube::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("glitchtip api error: {0}")]
    GlitchTipApi(#[from] crate::glitchtip::ApiError),

    #[error("secret {0} is missing or lacks required key {1}")]
    MissingSecretKey(String, String),

    #[error("referenced {kind} {namespace}/{name} not found")]
    RefNotFound {
        kind: &'static str,
        namespace: String,
        name: String,
    },

    #[error("waiting: {0}")]
    Waiting(String),

    #[error("invalid configuration: {0}")]
    Config(String),

    #[error("finalizer error: {0}")]
    Finalizer(#[source] Box<kube::runtime::finalizer::Error<Error>>),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
