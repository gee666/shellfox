// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { expect, it } from 'vitest';
import { fireEvent, render, screen, waitFor, cleanup } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { SettingsPanel } from './SettingsPanel';
import { createManagerClient } from './store';
import { mockApi, snapshot, profiles } from './test-fixtures';
it('shows Linux Python detection and autosaves through the existing settings controller', async()=>{
 const initial=snapshot();initial.probe.platform='linux';initial.probe.python={detected:'/usr/bin/python3',usable:true,reason:null};
 const f=mockApi(initial),client=createManagerClient(f.api);client.store.setState({snapshot:initial});
 render(<SettingsPanel client={client} settings={initial.settings} explorer={initial.explorer} cli={initial.cli} profiles={profiles} defaultProfileId={profiles[0].id}/>);
 const input=screen.getByLabelText('Python path');expect(input).toHaveAttribute('placeholder','/usr/bin/python3');fireEvent.change(input,{target:{value:'/opt/python3'}});
 await waitFor(()=>expect(f.api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({pythonPath:'/opt/python3'})),{timeout:2000});cleanup();client.stop();
});
it('keeps spaces while typing an absolute Python path', async()=>{
 const initial=snapshot();initial.probe.platform='linux';
 const f=mockApi(initial),client=createManagerClient(f.api);client.store.setState({snapshot:initial});
 render(<SettingsPanel client={client} settings={initial.settings} explorer={initial.explorer} cli={initial.cli} profiles={profiles} defaultProfileId={profiles[0].id}/>);
 const input=screen.getByLabelText('Python path'), user=userEvent.setup();
 await user.type(input,'/opt/Python tools/bin/python3');
 expect(input).toHaveValue('/opt/Python tools/bin/python3');
 await waitFor(()=>expect(f.api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({pythonPath:'/opt/Python tools/bin/python3'})),{timeout:2000});
 cleanup();client.stop();
});
it('does not show Python controls on Windows',()=>{
 const initial=snapshot(),f=mockApi(initial),client=createManagerClient(f.api);client.store.setState({snapshot:initial});
 render(<SettingsPanel client={client} settings={initial.settings} explorer={initial.explorer} cli={initial.cli} profiles={profiles} defaultProfileId={profiles[0].id}/>);
 expect(screen.queryByLabelText('Python path')).not.toBeInTheDocument();cleanup();client.stop();
});
