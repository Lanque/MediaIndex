# Local search and clip opening

Search runs against the SQLite index on the same machine. It does not need a
network connection and does not read media bytes again after indexing.

## Supported filters

The desktop search form supports:

- keyword: file name, path, container, codec, frame rate, and timestamp metadata;
- folder: a case-insensitive path fragment;
- date from/to: the local file modification date;
- resolution: exact `WIDTHxHEIGHT`, for example `1920x1080`;
- FPS: an exact FFprobe rate such as `30000/1001` or its decimal form `29.97`;
- duration min/max: seconds in the UI, converted to milliseconds in the index;
- codec: a fragment matching the video or audio codec.

## Example

To find a 29.97 FPS H.264 clip from the first shooting day:

1. Select the footage folder and wait for indexing to finish.
2. Enter `day-one` in Folder, `1920x1080` in Resolution, `29.97` in FPS,
   and `h264` in Codec.
3. Press **Apply filters** or use the keyword search.
4. Press **Preview** on an available result to play it in the MediaIndex
   window, or press **Open** to launch the original file with the operating
   system's associated player.

An unavailable result remains visible with an explicit status so a stale path
is distinguishable from a search miss. The Open button is disabled until a
future scan confirms that the local file is available again.

## Sorting

The desktop library can sort matching results by name, duration, file size,
modified date, or resolution, ascending or descending. Sorting happens over
the local SQLite result set and does not upload footage. Technical sort fields
are available after FFprobe metadata extraction; files whose metadata cannot be
read remain searchable by name and path and are shown with a warning.

The desktop UI renders at most 500 matching results at once. When a query
matches more than that, refine the keyword or filters instead of trying to
render the whole library in one window. Preview uses Tauri's local asset
protocol and never copies or uploads the original clip; codecs unsupported by
the embedded WebView show an actionable fallback message.

Initial hashing/FFprobe indexing, local database search, AI query embedding,
and AI connection tests run on background workers rather than the Tauri UI
thread. Selecting a new folder keeps **Analyze with AI** disabled until that
folder has been indexed successfully.

**Current folder** is always scoped to the folder most recently chosen with
**Select Footage Folder**. Selecting another folder does not mix earlier clips
into this view and does not mark clips from other indexed roots as missing.
**Analyzed archive** is the explicit cross-folder view: it lists only clips with
saved AI annotations and preserves their real source-folder grouping.

## AI search

AI search is a separate, explicit workflow:

1. Open **AI connection** and choose **Local (Ollama)**, **OpenAI API**,
   or **Google Gemini API**. Add the cloud API key when needed, verify the
   models/base URL, press **Test connection**, and save the settings. For
   OpenAI, choose GPT-5.6 Luna as the budget default or GPT-5.6 Terra only when
   recognition detail is worth roughly ten times Luna's model token price; use
   the [official model catalog](https://developers.openai.com/api/docs/models)
   as the source for current pricing.
   **Library context** is optional: use it to list a project/franchise, setting,
   or possible fictional characters for an unfamiliar collection. The model is
   instructed to use this only when it agrees with the visible evidence.
   The catalog also includes `gpt-4o-mini` and legacy `o4-mini`. A saved
   `04-mini` typo is normalized to the real `o4-mini` model ID.
2. Select a footage folder and wait for deterministic indexing to finish.
3. Press **Analyze with AI**. MediaIndex samples frames, stores descriptions
   and embeddings locally, and reports any per-file failures. Clips already
   analyzed with the selected provider/models are skipped by default so repeat
   clicks do not spend credits again. If every clip is already analyzed, the
   app asks before replacing anything. **Reanalyze existing clips** is an
   explicit one-run override and always warns that same-model annotations will
   be replaced and cloud requests may incur new cost.
4. Enter a natural-language query in the **AI Search** field, for example
   `Fortnite kill`, `enemy elimination`, or `player victory`.

AI results are ranked by embedding similarity and include the matching clip
time range. The index derives an adaptive merge window from that video's frame
sampling cadence. Semantically similar adjacent frames become one range, while
changes in action, setting, situation, visible dialogue, or other on-screen text
start a new moment.
Results are displayed as visual cards ordered by the best match for each video.
FFmpeg generates and caches a local thumbnail for the best matching timestamp;
clicking it opens Preview at that moment. Other moments are kept in a compact
chronological list without repeating the featured timestamp. **Focused** mode is the default and applies an adaptive
score window, an absolute relevance floor, and an eight-video/two-moment-per-video
cap. Exact or inflected on-screen text and labels receive enough ranking weight
to survive that floor, while generic embedding similarity alone is filtered.
**Balanced** and **Broad** intentionally show more exploratory matches. The default sampling
interval is five seconds. New OpenAI settings use a maximum of 60 frames per
file; other provider defaults remain 120. Use
the values in **AI connection** to adjust the cost/coverage trade-off. The full
original video is never uploaded as one file, but sampled frames are sent to the
configured provider. MediaIndex extracts all sampled frames for one clip in a
single FFmpeg pass and batches embedding requests. Cloud providers receive up
to eight sampled frames per vision request, sampled JPEGs are capped at 1280
pixels wide, and up to two clips are processed concurrently. This avoids
oversized parallel uploads while retaining high-detail HUD analysis; local
Ollama analysis remains sequential. The provider and model name are stored with
each annotation, so embeddings from incompatible models are not mixed in one
search.

Before any analysis, MediaIndex performs a local preflight and asks for
confirmation with the unique-video count, duration-based estimated sampled
frames/vision requests, and configured upper bounds. Identical content found at
multiple paths is counted and analyzed once. Cloud preflights also show the
upload/request bounds. The first completed run calibrates
a machine-local per-model timing estimate; later preflights show that estimate,
and an in-progress run shows a live ETA. Cancelling confirmation sends no API
request.

The vision prompt requests recognizable fictional characters and franchises,
other visible entities, actions and interactions, the setting, the broader
situation, and readable text from HUDs, subtitles, signs, and overlays. Visible
subtitle/caption dialogue is indexed separately. Frame-only analysis cannot
hear speech that is absent from the image; audio transcription is a separate
future pipeline. It does
not identify a real person from their face alone. Search ranking combines
semantic similarity with keyword matching and light inflection handling
(`kill`/`killed`, `elimination`/`eliminated`). For short-lived events, set
**Every (s)** to `2` or `1` before analysis. Changing provider, vision model, or
embedding model requires analyzing the folder again with that configuration;
otherwise the app reports that no AI moments exist for the active model
namespace.

AI analysis runs in background workers and reports the current phase, clip, and
overall 0–100% completion through the progress bar, so the desktop window
remains interactive during long analyses.

For **Local (Ollama)**, Ollama must be running at the configured base URL and
the selected models must already be installed. For cloud providers, the app
uses the API directly; a ChatGPT web subscription is not an API key. The API
key is kept only for the current app session and is not written to the
repository or persistent browser storage.
