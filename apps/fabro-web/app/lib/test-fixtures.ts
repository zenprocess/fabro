import type { Principal } from "@qltysh/fabro-api-client";

export const TEST_PRINCIPAL = {
  kind:        "user",
  identity:    { issuer: "fabro:test", subject: "test-user" },
  login:       "test",
  auth_method: "dev_token",
} satisfies Principal;
