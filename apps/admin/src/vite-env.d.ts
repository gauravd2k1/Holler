/// <reference types="vite/client" />

// The two settings this app refuses to default. Declared so `import.meta.env`
// is typed rather than `any`: a typo in a variable name should be a compile
// error, not an empty string that reaches the config check at runtime.
interface ImportMetaEnv {
  readonly VITE_ADMIN_API_BASE_URL?: string;
  readonly VITE_ADMIN_OUTLET_ID?: string;
  readonly VITE_ADMIN_TENANT_ID?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
