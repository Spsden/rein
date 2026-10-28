# OpenCode Go

Use an existing Go subscription/API key; Rein never purchases a plan, changes
billing settings or enables extra usage. Keep your key out of source control.

```sh
export OPENCODE_GO_API_KEY='your-key'
cargo run --locked -- go /tmp/rein-go.json YOUR_MODEL 'Help me plan a Rust change'
```

Use a model ID currently listed by the provider. The Go adapter targets
`https://opencode.ai/inference/go/openai/v1/chat/completions`, sends
`User-Agent: rein/0.1.0` and binds `x-opencode-session` to the saved session ID.
Resuming the same session preserves that header across invocations. HTTP redirects
are disabled on conversation-scoped transports. Credentials remain environment
configuration, never snapshot contents.

The `go` command currently grants only echo; the coding mode is documented
separately when available. This is provider integration, not a benchmark score.

Endpoint and identity requirements were verified against the
[official OpenCode Go documentation](https://opencode.ai/v2/docs/console/go/)
on 2026-10-28. Provider availability, quotas and models may change. Live evaluation
requires a configured key and must run only against disposable task workspaces.
