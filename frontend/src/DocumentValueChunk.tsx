import React, {
  useEffect,
} from 'react';
import { logPerformance } from './utils/performance';

const DocumentValueChunk = React.memo(function DocumentValueChunk(props: any) {
  let {id, version, model} = props;
  const value = model.getChunkValue(id);
  return (
    <div className="DocumentValueChunk">
      {value}
    </div>
  );
});

export default DocumentValueChunk;
