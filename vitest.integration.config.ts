import { integrationPaths } from './tests/config/integration-registry';

function refuseUnownedIntegration(paths: readonly string[]): never {
  // No configuration that could collect these files exists until Root approves original owners.
  throw new Error(`OWNED_INTEGRATION_HOLD: ${paths.length} registered files. Independent OS owners and Go3 original-root capture are unavailable.`);
}
export default refuseUnownedIntegration(integrationPaths);
