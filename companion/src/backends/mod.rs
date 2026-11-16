//! The services that can translate a line. Each takes the settings, the text and the
//! direction, and returns the service's raw answer; `translate::translate` picks one and
//! tidies the answer.

pub mod claude;
pub mod google;
pub mod openai;
