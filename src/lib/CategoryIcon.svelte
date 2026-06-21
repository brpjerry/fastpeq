<script lang="ts">
  // Category icons are discrete SVG files in ./icons/. Each is monochrome
  // line-art drawn with `currentColor` (no fill), so it follows the surrounding
  // text colour and inverts cleanly for light/dark. To add a category icon, drop
  // a `<value>.svg` into that folder — it's matched to the `category` prop by
  // file name and picked up automatically (no edits here needed).
  const files = import.meta.glob<string>("./icons/*.svg", {
    query: "?raw",
    import: "default",
    eager: true,
  });
  const icons: Record<string, string> = {};
  for (const [path, svg] of Object.entries(files)) {
    const name = path.split("/").pop()!.replace(/\.svg$/, "");
    // Drop the XML prolog some editors (Inkscape) add — it's invalid in inline HTML.
    icons[name] = svg.replace(/<\?xml[\s\S]*?\?>/, "").trim();
  }

  let { category }: { category?: string } = $props();
  const svg = $derived(icons[category ?? "uncategorized"] ?? icons.uncategorized ?? "");
</script>

<span class="ci">{@html svg}</span>

<style>
  .ci {
    display: inline-flex;
    line-height: 0;
  }
  .ci :global(svg) {
    width: 16px;
    height: 16px;
    display: block;
  }
</style>
