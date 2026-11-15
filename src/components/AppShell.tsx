import { NavLink, Outlet } from 'react-router-dom';
import { CheckCircle2, CircleGauge, FileText, Images, Settings2 } from 'lucide-react';

const navigation = [
  { label: 'Add', to: '/add', Icon: Images },
  { label: 'Preferences', to: '/preferences', Icon: Settings2 },
  { label: 'Progress', to: '/progress', Icon: CircleGauge },
  { label: 'Done', to: '/done', Icon: CheckCircle2 },
  { label: 'Report', to: '/report', Icon: FileText },
  { label: 'Review', to: '/review', Icon: Images },
];

export default function AppShell() {
  return (
    <div className="app-shell flex min-h-screen min-w-[960px] text-ink">
      <aside className="app-navigation" aria-label="Main navigation">
        <div className="app-brand">
          <svg
            aria-hidden="true"
            className="app-brand__mark"
            viewBox="0 0 16 16"
            fill="none"
          >
            <rect width="16" height="16" rx="4" fill="var(--tb-color-accent)" />
            <circle
              cx="8"
              cy="8"
              r="3"
              stroke="var(--tb-color-accent-ink)"
              strokeWidth="1.6"
            />
          </svg>
          <span className="font-semibold">Takeback</span>
        </div>
        <nav className="app-navigation__items">
          {navigation.map(({ label, to, Icon }) => (
            <NavLink key={to} to={to} className="app-navigation__link">
              <Icon size={18} strokeWidth={1.7} className="app-navigation__icon" aria-hidden="true" />
              <span>{label}</span>
            </NavLink>
          ))}
        </nav>
      </aside>
      <main className="app-main" aria-label="Page content">
        <Outlet />
      </main>
    </div>
  );
}
