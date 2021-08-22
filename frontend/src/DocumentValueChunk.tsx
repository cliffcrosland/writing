import React, {
  useEffect,
  useMemo,
  useState
} from 'react';
import { logPerformance } from './utils/performance';

function DocumentValueChunk(props: any) {
  let {id, version, model} = props;
  // Only recompute chunk value when id or version change.
  const memoizedValue = useMemo(
    () => {
      console.log(`getting value for chunk id: ${id}, version: ${version}`);
      return logPerformance('getChunkValue', () => model.getChunkValue(id));
    },
    [id, version]
  );
  return (
    <div className="DocumentValueChunk">
      {memoizedValue}
    </div>
  );
}

export default DocumentValueChunk;
