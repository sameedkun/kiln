// A site's framework at a glance (P4C-04): Kiln's own monogram on a tint of a hue per
// framework. Frameworks' logos are their owners' marks, so none is drawn here. The letters
// stay in the text colour, so contrast holds in light, dark and high-contrast modes.

const marks: Record<string, { letters: string; hue: string }> = {
  laravel: { letters: "Lv", hue: "#e5533d" },
  statamic: { letters: "St", hue: "#ff269e" },
  symfony: { letters: "Sf", hue: "#7d8b99" },
  drupal: { letters: "Dr", hue: "#2b8ad6" },
  craft: { letters: "Cr", hue: "#e5422b" },
  "wordpress-bedrock": { letters: "Bd", hue: "#5a8f3c" },
  wordpress: { letters: "Wp", hue: "#3d7fb8" },
  php: { letters: "Php", hue: "#7a86b8" },
  static: { letters: "Ht", hue: "#c9853a" },
};

/** The mark for a framework id (the CLI's `framework`, or a driver name). */
export function markOf(framework: string): { letters: string; hue: string } {
  return marks[framework] ?? marks.php;
}

export function FrameworkMark({ framework, label }: { framework: string; label: string }) {
  const { letters, hue } = markOf(framework);
  return (
    <span
      role="img"
      aria-label={label}
      title={label}
      className="inline-grid h-[22px] min-w-[22px] shrink-0 place-items-center rounded-md px-1 text-[10.5px] font-semibold tracking-[-.01em] text-text forced-colors:border forced-colors:border-[CanvasText]"
      style={{ background: `color-mix(in oklch, ${hue} 26%, transparent)`, boxShadow: `inset 0 0 0 1px color-mix(in oklch, ${hue} 45%, transparent)` }}
    >
      {letters}
    </span>
  );
}
