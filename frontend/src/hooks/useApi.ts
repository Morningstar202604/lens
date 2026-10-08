//! Dead-code placeholder.
//!
//! This hook was defined but never used by any component.
//! `TimelineView` / `DashboardView` / `Sidebar` each fetch their own data
//! through the imperative helpers in `lib/api.ts`.  Keeping this module only
//! to silence the `unused` warning would be misleading — it is not wired up.
//!
//! TODO: adopt `useApi` (or a Query library like `@tanstack/react-query`) across
//! all fetchers for consistent caching / loading / error state.
export {};
