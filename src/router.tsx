import { createMemoryRouter, Navigate } from 'react-router-dom';
import AppShell from './components/AppShell';
import { AddPage, DonePage, PreferencesPage, ProgressPage, ReportPage, ReviewPage } from './pages';

export const router = createMemoryRouter([
  {
    path: '/',
    element: <AppShell />,
    children: [
      { index: true, element: <Navigate to="/add" replace /> },
      { path: 'add', element: <AddPage /> },
      { path: 'preferences', element: <PreferencesPage /> },
      { path: 'progress', element: <ProgressPage /> },
      { path: 'done', element: <DonePage /> },
      { path: 'report', element: <ReportPage /> },
      { path: 'review', element: <ReviewPage /> },
    ],
  },
]);
