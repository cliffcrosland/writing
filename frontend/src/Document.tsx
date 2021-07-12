import React, { useState } from 'react';
import DocumentEditor from './DocumentEditor';
import OtDebuggingServer from './OtDebuggingServer';
import './Document.css';

function Document() {
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
    <div className="Document">
      <h1 className="Document-header">Document</h1>
      <div className="Document-body">
        <div className="Document-clients">
          {clientIds.map((clientId) => 
            <DocumentEditor key={clientId} clientId={clientId} />
          )}
          <button className="Document-addClient" onClick={onAddClientClick}>
            Add Client
          </button>
        </div>
        <div className="Document-server">
          <OtDebuggingServer />
        </div>
      </div>
    </div>
  );
}

export default Document;
