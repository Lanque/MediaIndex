# AI provider compatibility

Compatibility snapshot: 2026-09-03. Model catalogs and provider authentication
rules change independently of the desktop release, so recheck the linked
official documentation before changing these defaults.

## Supported defaults

| Provider | Vision model | Embedding model | Authentication and notes |
| --- | --- | --- | --- |
| OpenAI API | `gpt-5.6-luna` | `text-embedding-3-small` | Bearer API key. Luna is the cost-conscious desktop default; optional timestamped speech uses `whisper-1`. |
| Google Gemini API | `gemini-3.8-flash` | `gemini-embedding-2` | AI Studio key (`x-goog-api-key`) or Google Desktop OAuth (`Bearer` + quota-project header). Embeddings use 768 output dimensions and retrieval-oriented text prefixes. |
| Local Ollama | `gemma4:e2b` | `embeddinggemma` | No cloud key. Both models must exist in the configured Ollama instance. |

The OpenAI picker also exposes low-cost `gpt-4o-mini` and legacy `o4-mini` for
existing indexes. The common `04-mini` (zero-four) typo is normalized to
`o4-mini`; it is not a separate model.

The in-app catalog shows provider token prices where an official token price is
available. It deliberately does not promise a price per clip: that depends on
video duration, sampling interval, frame grouping, image tokenization, retries,
and the selected model. OpenAI preflight separately reports how much compressed
speech audio will be sent for timestamped transcription.

## Connection validation

**Test connection** validates both halves of the selected configuration:

1. the vision model exists and is accessible; and
2. the embedding endpoint returns a non-empty numeric vector.

This prevents a successful embedding-only check from hiding a missing or
unauthorized vision model until a long analysis run.

## Gemini authentication and migration

MediaIndex sends Gemini keys only in the `x-goog-api-key` header. Keys are not
put into query strings or logs. The desktop accepts a Gemini API key created in
Google AI Studio; legacy standard Google Cloud API keys, including unrestricted
keys, may be rejected as Google completes its Gemini key migration.

The native desktop app also provides **Login with Google**. Google requires the
owner of the API project to create that client registration, so first create a
Google Cloud project, enable the Generative Language API, configure its OAuth
consent screen, create a **Desktop app** OAuth client, and download the client
JSON. Select that JSON when MediaIndex asks for it. MediaIndex uses PKCE and an
ephemeral loopback callback on `127.0.0.1`; the access token stays in process
memory, is never written to settings, and is cleared on disconnect, expiry, or
app exit. Gemini OAuth requests use the JSON's project ID as the quota project.

MediaIndex does not present a fake provider login. OpenAI's API uses developer
API-key Bearer authentication; a ChatGPT account session or subscription is not
an API credential, and OpenAI does not expose a third-party **Login with
ChatGPT** flow for this API use case.

Saved `text-embedding-004` and `embedding-001` selections are migrated to
`gemini-embedding-2`. The retired names are not used as silent fallbacks. Gemini
Embedding 2 requests use a fixed 768-dimensional output and format query and
document text for retrieval. This keeps one model's vectors structurally
consistent across indexing and search.

## Embedding namespace rule

Vectors from different providers, models, or dimensions are incompatible.
MediaIndex therefore stores annotations under a provider/vision/embedding model
namespace. After changing any member of that tuple, run **Analyze with AI** with
**Reanalyze existing clips** enabled once. Old annotations remain separate and
are not mixed into the new search space.

## Failure recovery

| Failure | Recommended action |
| --- | --- |
| OpenAI 401/403 | Check that the value is an OpenAI developer API key and that the account/project can use the selected model. |
| OpenAI 404 | Re-select a model from the current OpenAI API catalog; do not use a ChatGPT product name as an API model ID. |
| Gemini 401/403 | Verify the AI Studio key, or ensure the OAuth JSON's Cloud project has the Generative Language API, consent, quota, and model access configured. |
| Gemini 404 | Re-select the exact current Gemini model ID; retired embedding names are unsupported. |
| Ollama 404 | Pull the exact selected model, for example `ollama pull gemma4:e2b` and `ollama pull embeddinggemma`. |
| Network send error | Check connectivity, proxy/firewall settings, provider base URL, and system clock, then retry **Test connection**. |

Automated provider tests use local HTTP stubs. They do not contain real API keys
or consume provider credits.

## Official references

- [OpenAI API model catalog](https://developers.openai.com/api/docs/models)
- [OpenAI `gpt-4o-mini`](https://developers.openai.com/api/docs/models/gpt-4o-mini)
- [OpenAI `o4-mini`](https://developers.openai.com/api/docs/models/o4-mini)
- [OpenAI `text-embedding-3-small`](https://developers.openai.com/api/docs/models/text-embedding-3-small)
- [OpenAI speech-to-text](https://developers.openai.com/api/docs/guides/speech-to-text)
- [Gemini API keys](https://ai.google.dev/gemini-api/docs/api-key)
- [Gemini API authentication](https://ai.google.dev/api)
- [Gemini OAuth](https://ai.google.dev/gemini-api/docs/oauth)
- [Gemini embeddings](https://ai.google.dev/gemini-api/docs/embeddings)
- [Gemini Embedding 2](https://ai.google.dev/gemini-api/docs/models/gemini-embedding-2)
- [Ollama Gemma 4](https://ollama.com/library/gemma4)
- [Ollama EmbeddingGemma](https://ollama.com/library/embeddinggemma)
