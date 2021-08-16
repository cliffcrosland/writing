import React from 'react';
import { useParams } from 'react-router-dom';
import DocumentEditor from './DocumentEditor';
import './Document.css';

function Document() {
  let { id } = useParams() as any;

  return (
    <div className="Document">
      <DocumentEditor docId={id} />
    </div>
  );
}

export default Document;
