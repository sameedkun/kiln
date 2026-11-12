// Kiln's mark: a kiln's dome with its door, and an ember inside.
export function Logo({ size = 18, title }: { size?: number; title?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      role={title ? "img" : undefined}
      aria-label={title}
      aria-hidden={title ? undefined : true}
      className="shrink-0"
    >
      <path d="M4 21V11.5a8 8 0 0 1 16 0V21z" fill="currentColor" opacity=".16" />
      <path d="M4 21V11.5a8 8 0 0 1 16 0V21" fill="none" stroke="currentColor" strokeWidth="1.7" />
      <path d="M9 21v-4.5a3 3 0 0 1 6 0V21" fill="none" stroke="currentColor" strokeWidth="1.7" />
      <path
        d="M12 9.2c1.2 1 1.6 1.9 1.2 2.7-.3.6-.8.9-1.2.9s-.9-.3-1.2-.9c-.4-.8 0-1.7 1.2-2.7z"
        fill="#ea580c"
      />
      <path d="M2.5 21h19" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" />
    </svg>
  );
}
