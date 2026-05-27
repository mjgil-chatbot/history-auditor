pub mod service {
    use anyhow::{bail, Result};

    use crate::session::{InspectedSession, SessionSource};

    #[derive(Clone, Debug, Default)]
    pub struct SessionMcpService;

    impl SessionMcpService {
        pub fn new(_stores: Vec<()>) -> Self {
            Self
        }
    }

    impl SessionSource for SessionMcpService {
        fn inspect_session(&self, session_id: &str) -> Result<InspectedSession> {
            bail!("No fixture session source configured for `{session_id}`")
        }
    }
}
