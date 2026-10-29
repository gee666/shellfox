import React from 'react';
import type { ShellfoxModel } from './useShellfox';
import { Button, Note } from './ui';

export function SessionsEmptyState({ model, settings }: { model: ShellfoxModel; settings: () => void }) {
  if (model.network.status === 'error' || model.preferences.remembered.length > 0 && model.network.status === 'online') {
    return <>
      <Note error={model.network.status === 'error'}>
        {model.network.status === 'error' ? model.network.error ?? 'Could not load connections.' : 'Saved machines are unavailable.'}
      </Note>
      <Button title="Retry" onPress={() => { void model.retryStart().catch(model.report); }} />
    </>;
  }
  return <Button title="Open Settings" onPress={settings} />;
}
