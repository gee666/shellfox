import type { z } from 'zod';
import type { hostEndpointSchema, hostEventSchema, hostRequestSchemas, hostResponseSchemas, hostStatusSchema } from './host-schemas';
export type HostMode = 'desktop' | 'headless';
export type HostStatus = z.infer<typeof hostStatusSchema>;
export type HostEndpoint = z.infer<typeof hostEndpointSchema>;
export type HostEvent = z.infer<typeof hostEventSchema>;
export type HostMethod = keyof typeof hostRequestSchemas;
export type HostRequest<M extends HostMethod> = z.input<(typeof hostRequestSchemas)[M]>;
export type HostResponse<M extends HostMethod> = z.output<(typeof hostResponseSchemas)[M]>;
