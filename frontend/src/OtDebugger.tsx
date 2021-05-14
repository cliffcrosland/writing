import React, { useState } from 'react';
import OtClient from './OtClient';
import OtServer from './OtServer';
import './OtDebugger.css';

function OtDebugger() {
  const [clientIds, setClientIds] = useState(() => [1]);

  function onAddClientClick() {
    setClientIds((clientIds: Array<number>) => {
      if (clientIds.length === 0) {
        return [1];
      } else {
        return [...clientIds, clientIds[clientIds.length - 1] + 1];
      }
    });
  }

  return (
    <div className="OtDebugger">
      <h1 className="OtDebugger-header">OtDebugger</h1>
      <div className="OtDebugger-body">
        <div className="OtDebugger-clients">
          {clientIds.map((clientId) => 
            <OtClient key={clientId} clientId={clientId} />
          )}
          <button onClick={onAddClientClick}>
            Add Client
          </button>
        </div>
        <div className="OtDebugger-server">
          <OtServer />
        </div>
      </div>
    </div>
  );
}

export default OtDebugger;
