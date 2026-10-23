//! The moderation lane: receives every platform message, screens it ([`super::rules`]),
//! carries decisions out when armed, logs them ([`super::store`]), and asks
//! [`super::voice`] for the line. Runs whenever a platform is connected, show or no show.
