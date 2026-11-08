# Privacy

Rubrica collects nothing about you or your readers.

- **No telemetry.** Rubrica sends no usage data, crash reports or identifiers anywhere.
- **No update checks.** Rubrica never contacts a server to ask whether a newer version exists.
- **No remote fonts or scripts.** The pages `rubrica build` writes load nothing from another
  host: the stylesheet, the font and any script are files in the site's own `_assets/`.
- **Nothing that phones home.** The output of `rubrica build` is plain files; it contains no
  tracker, beacon or analytics.
- **`rubrica serve`** (from 0.2) logs what the operator configures. By default it logs nothing
  beyond status lines.

Whatever you add to your own pages is yours, and so is its effect on your readers' privacy:
a page that embeds an image from another host makes the reader's browser fetch it.
