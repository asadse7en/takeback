import { useEffect, useState } from 'react';
import { RouterProvider } from 'react-router-dom';
import { Button } from './components/Ui';
import { callCommand } from './lib/api';
import { router } from './router';
import { useSettingsStore } from './store/settings';
import type { Settings } from './types/app';

let settingsLoadPromise: Promise<Settings> | null = null;

function loadSettingsOnce() {
  if (settingsLoadPromise === null) {
    settingsLoadPromise = callCommand('load_settings', {}).catch((reason: unknown) => {
      settingsLoadPromise = null;
      throw reason;
    });
  }
  return settingsLoadPromise;
}

export default function App() {
  const settingsLoaded = useSettingsStore((state) => state.settings !== null);
  const replaceSettings = useSettingsStore((state) => state.replaceSettings);
  const [loadError, setLoadError] = useState('');
  const [loadAttempt, setLoadAttempt] = useState(0);

  useEffect(() => {
    let active = true;
    void loadSettingsOnce()
      .then(replaceSettings)
      .catch((reason: unknown) => {
        if (active) setLoadError(reason instanceof Error ? reason.message : String(reason));
      });
    return () => {
      active = false;
    };
  }, [loadAttempt, replaceSettings]);

  if (!settingsLoaded) {
    return (
      <div className="settings-load-state" role={loadError ? 'alert' : 'status'} aria-live="polite">
        <p>{loadError ? `Could not load preferences: ${loadError}` : 'Loading preferences…'}</p>
        {loadError ? (
          <Button onClick={() => {
            setLoadError('');
            setLoadAttempt((attempt) => attempt + 1);
          }}>Retry</Button>
        ) : null}
      </div>
    );
  }

  return <RouterProvider router={router} />;
}
