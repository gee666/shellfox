import React, { useState } from 'react';
import type { EnvVar } from '../../src/shared/contracts';
import type { MachineView, ShellfoxModel } from './useShellfox';
import { EnvironmentEditor } from './EnvironmentEditor';
import { Button } from './ui';

export function HostEnvironmentSettings({ machine, refresh, disabled }: {
  machine: MachineView;
  refresh: ShellfoxModel['refresh'];
  disabled: boolean;
}) {
  const [environment, setEnvironment] = useState<EnvVar[] | null>(null);
  return (
    <>
      <Button
        title="Add Environment Variables (all terminals)"
        subtle
        disabled={disabled}
        onPress={() => setEnvironment(machine.snapshot?.settings.terminalEnv ?? [])}
      />
      {environment && (
        <EnvironmentEditor
          initialEnv={environment}
          title={`Environment on ${machine.peer.hostname}`}
          description={`Applies to new terminals on ${machine.peer.hostname}. Session variables override these values.`}
          close={() => setEnvironment(null)}
          save={async env => {
            const result = await machine.client.setTerminalEnv({ env });
            if (!result.ok) throw new Error(result.error.message);
            await refresh(machine.client);
          }}
        />
      )}
    </>
  );
}
