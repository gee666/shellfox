// Deliberately imports no runner, test, app, helper or native module.
process.stderr.write('OWNED_INTEGRATION_HOLD: no approved original-owner attachment; Go3 original-root capture unresolved. Nothing launched.\n');
process.exitCode = 2;
