mod error;
mod id;
mod model;
mod store;

pub use error::{AutomationStoreError, AutomationValidationError};
pub use id::{AutomationId, AutomationTriggerId};
pub use model::{
    ApiTrigger, Automation, AutomationDraft, AutomationPatch, AutomationReplace,
    AutomationRevision, AutomationTarget, AutomationTrigger, GitRefSelector, RepositorySlug,
    ScheduleTrigger, WorkflowSlug,
};
pub use store::AutomationStore;
