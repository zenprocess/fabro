import type { Principal } from "@qltysh/fabro-api-client";

export function testPrincipal(login = "test"): Principal {
  return {
    kind:        "user",
    identity:    { issuer: "fabro:test", subject: `${login}-user` },
    login,
    auth_method: "dev_token",
  };
}
