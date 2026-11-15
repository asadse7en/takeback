import type { ReactNode } from 'react';

export function PageFrame({
  title,
  subtitle,
  children,
  footer,
  titleAdornment,
  className = '',
}: {
  title: string;
  subtitle?: string;
  children: ReactNode;
  footer: ReactNode;
  titleAdornment?: ReactNode;
  className?: string;
}) {
  return (
    <div className={`page-frame${className ? ` ${className}` : ''}`}>
      <div className="page-content">
        <header className="page-heading">
          {titleAdornment}
          <div>
            <h1>{title}</h1>
            {subtitle ? <p>{subtitle}</p> : null}
          </div>
        </header>
        {children}
      </div>
      <footer className="page-footer">{footer}</footer>
    </div>
  );
}

export function Panel({
  children,
  className = '',
}: {
  children: ReactNode;
  className?: string;
}) {
  return <section className={`panel ${className}`}>{children}</section>;
}

export function Button({
  children,
  onClick,
  variant = 'default',
  className = '',
  disabled = false,
  type = 'button',
}: {
  children: ReactNode;
  onClick?: () => void;
  variant?: 'default' | 'accent';
  className?: string;
  disabled?: boolean;
  type?: 'button' | 'submit';
}) {
  return (
    <button
      className={`ui-button${variant === 'accent' ? ' ui-button--accent' : ''}${className ? ` ${className}` : ''}`}
      type={type}
      onClick={onClick}
      disabled={disabled}
    >
      {children}
    </button>
  );
}

export function PageHeading({ children }: { children: ReactNode }) {
  return <h2 className="section-heading">{children}</h2>;
}

export function SegmentedControl({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string;
  options: readonly string[];
  onChange: (value: string) => void;
}) {
  return (
    <div className="segmented-control" role="group" aria-label={label}>
      {options.map((option) => (
        <button
          key={option}
          type="button"
          className={option === value ? 'is-selected' : ''}
          aria-pressed={option === value}
          onClick={() => onChange(option)}
        >
          {option}
        </button>
      ))}
    </div>
  );
}

export function Toggle({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <button
      className={`ui-toggle${checked ? ' is-on' : ''}`}
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
    >
      <span />
    </button>
  );
}

const landscapeColors = {
  peach: ['#F6E3CF', '#F7B15F', '#D9A06F', '#B87A4C'],
  blue: ['#D6E6F2', '#F5E1A8', '#8FB3CC', '#5E88A8'],
  green: ['#E4EEDC', '#F6D98B', '#A3C08A', '#6F9A63'],
  rose: ['#EAD9E4', '#F4B7A0', '#B79AB5', '#8A6F93'],
} as const;

export function PhotoThumb({
  tone,
  className = '',
}: {
  tone: keyof typeof landscapeColors;
  className?: string;
}) {
  const [background, sun, hill, foreground] = landscapeColors[tone];

  return (
    <svg
      className={`photo-thumb ${className}`}
      viewBox="0 0 150 100"
      preserveAspectRatio="xMidYMid slice"
      aria-hidden="true"
    >
      <rect width="150" height="100" fill={background} />
      <circle cx="110" cy="30" r="12" fill={sun} />
      <path d="M0 100V70Q40 45 80 68T150 62V100z" fill={hill} />
      <path d="M0 100V82Q50 66 100 84T150 80V100z" fill={foreground} />
    </svg>
  );
}

export function StatusCheck({ children }: { children: ReactNode }) {
  return (
    <span className="status-check">
      <svg viewBox="0 0 20 20" fill="none" aria-hidden="true">
        <circle cx="10" cy="10" r="8" />
        <path d="m6.5 10.2 2.4 2.4 4.6-5" />
      </svg>
      {children}
    </span>
  );
}
