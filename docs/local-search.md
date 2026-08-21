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

## AI search

AI search is a separate, explicit workflow:

1. Open **AI connection** and choose **Local (Ollama)**, **OpenAI (ChatGPT API)**,
   or **Google Gemini API**. Add the cloud API key when needed, verify the
   models/base URL, press **Test connection**, and save the settings. For
   OpenAI, choose GPT-5.6 Luna for faster high-volume analysis or GPT-5.6 Terra
   when recognition detail is more important than speed and cost.
   **Library context** is optional: use it to list a project/franchise, setting,
   or possible fictional characters for an unfamiliar collection. The model is
   instructed to use this only when it agrees with the visible evidence.
2. Select a footage folder and wait for deterministic indexing to finish.
3. Press **Analyze with AI**. MediaIndex samples frames, stores descriptions
   and embeddings locally, and reports any per-file failures.
4. Enter a natural-language query in the **AI Search** field, for example
   `Fortnite kill`, `enemy elimination`, or `player victory`.

AI results are ranked by embedding similarity and include the matching clip
timestamp. Adjacent annotations from the same video within three seconds are
coalesced after ranking, retaining the best-scoring timestamp for that event.
Results are displayed in collapsible video groups: videos remain ordered by
their best match, while each video's matching moments are ordered by timestamp.
Press **Preview** to open the clip at that timestamp. The default
sampling interval is five seconds with a maximum of 120 frames per file; use
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

The vision prompt requests recognizable fictional characters and franchises,
other visible entities, actions and interactions, the setting, the broader
situation, and readable text from HUDs, subtitles, signs, and overlays. It does
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
key is kept in local app storage and is not written to the repository.
