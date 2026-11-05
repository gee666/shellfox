import type { FileList } from '../../shared/file-contracts';

export interface FileDirectoryRequest {
  path: string;
  id: string | number;
  listing?: FileList;
}
