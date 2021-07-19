import React, { useState } from 'react';
import DocumentEditor from './DocumentEditor';
import './Document.css';

function Document() {
  const orgId = "o-abc123";
  const docId = "d-def456";
  const userId = "u-ghi789";

  return (
    <div className="Document">
      <h1 className="Document-header">Document</h1>
      <DocumentEditor orgId={orgId} docId={docId} userId={userId} />
    </div>
  );
}

export default Document;
