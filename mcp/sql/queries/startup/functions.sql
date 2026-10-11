select fn, has_function_privilege(fn, 'execute') as ok from (values
  ('flybase.get_stocks(text)'), ('gene_group.is_pathway(text, character varying)')) v(fn)
order by 1
