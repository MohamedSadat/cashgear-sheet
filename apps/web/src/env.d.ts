/// <reference types="vite/client" />

/** The app's version, from the root package.json (vite.config.ts). */
declare const __CGS_VERSION__: string;

interface ImportMetaEnv {
  /** Cash Gear ERP's API origin, when a build names one (`packages/erp-connector/src/launch.ts`). */
  readonly VITE_ERP_API_URL?: string;
  /** Cash Gear Workspace's API origin, when a build names one (`@cgs/workspace-connector`). */
  readonly VITE_WORKSPACE_API_URL?: string;
}
