typedef void* video_player_ptr;

// Créé une instance du lecteur
//
// Renvoie NULL si une erreur s'est produite,
// le pointeur du lecteur sinon
video_player_ptr aml_video_player_create();

// Charge la vidéo depuis l'URL donnée
// l'URL peut être une adresse web délivrant
// du mp4 valide,
// ou un chemin sur le système de fichier courant
//
// Renvoie <0 en cas d'erreur
int aml_video_player_load(video_player_ptr, const char* video_url);

// Détruit l'instance du lecteur vidéo. Utiliser
// ce pointeur par la suite est un comportement
// indéfini.
//
int aml_video_player_destroy(video_player_ptr);
