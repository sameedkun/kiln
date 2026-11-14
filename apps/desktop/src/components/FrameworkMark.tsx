import { siCraftcms, siDrupal, siHtml5, siLaravel, siPhp, siRoots, siStatamic, siSymfony, siWordpress, type SimpleIcon } from "simple-icons";

// A site's framework at a glance (P4D-04): the framework's own icon, from Simple Icons
// (CC0), in its brand colour on a quiet tile. A brand colour too dark or too light to see on
// both backgrounds (Symfony's black) is drawn in the text colour instead.

const icons: Record<string, SimpleIcon> = {
  laravel: siLaravel,
  statamic: siStatamic,
  symfony: siSymfony,
  drupal: siDrupal,
  craft: siCraftcms,
  "wordpress-bedrock": siRoots,
  wordpress: siWordpress,
  php: siPhp,
  static: siHtml5,
};

/** The icon for a framework id (the CLI's `framework`, or a driver name). */
export function iconOf(framework: string): SimpleIcon {
  return icons[framework] ?? siPhp;
}

/** Whether a brand colour reads on both light and dark tiles: its luminance in the middle. */
export function readable(hex: string): boolean {
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const l = 0.2126 * r + 0.7152 * g + 0.0722 * b;
  return l > 0.12 && l < 0.85;
}

export function FrameworkMark({ framework, label }: { framework: string; label: string }) {
  const icon = iconOf(framework);
  const colour = readable(icon.hex) ? `#${icon.hex}` : "currentColor";
  return (
    <span
      role="img"
      aria-label={label}
      title={label}
      className="inline-grid size-[22px] shrink-0 place-items-center rounded-md bg-hover text-text forced-colors:border forced-colors:border-[CanvasText]"
    >
      <svg viewBox="0 0 24 24" aria-hidden="true" className="size-[14px] forced-colors:fill-[CanvasText]" fill={colour}>
        <path d={icon.path} />
      </svg>
    </span>
  );
}
